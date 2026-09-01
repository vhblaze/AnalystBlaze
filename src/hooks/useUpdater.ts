import { useCallback, useEffect, useState } from "react";
import { toast } from "@/hooks/use-toast";
import { useI18n } from "@/i18n";
import {
  applyUpdate,
  checkForUpdate,
  dismissUpdate,
  getUpdateStatus,
  listenToUpdateStatus,
  type UpdateStatus,
} from "@/services/tauri/agent";

export function useUpdater() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const { t } = useI18n();

  useEffect(() => {
    let active = true;
    let dispose: (() => void) | undefined;

    // `lastInstallOutcome` is a one-shot field the Rust side clears the moment
    // this call reads it - it's only ever populated right after startup, when
    // reconciling the update the user consented to install last session. This
    // is the only place it's ever shown, otherwise a failed install would
    // just silently re-show the same "update available" prompt with no
    // explanation.
    getUpdateStatus()
      .then((next) => {
        if (active) setStatus(next);
        const outcome = next.lastInstallOutcome;
        if (!outcome) return;
        if (outcome.kind === "succeeded") {
          toast({
            title: t("update.installSucceededToastTitle"),
            description: t("update.installSucceededToastDesc", { version: outcome.version }),
          });
        } else {
          toast({
            title: t("update.installFailedToastTitle"),
            description: t("update.installFailedToastDesc", { version: outcome.runningVersion }),
            variant: "destructive",
          });
        }
      })
      .catch(() => undefined);

    listenToUpdateStatus((next) => {
      if (active) setStatus(next);
    }).then((disposer) => {
      dispose = disposer;
      if (!active) disposer();
    });

    return () => {
      active = false;
      dispose?.();
    };
  }, []);

  const check = useCallback(async () => {
    const next = await checkForUpdate();
    setStatus(next);
    return next;
  }, []);

  const apply = useCallback(async () => {
    const next = await applyUpdate();
    setStatus(next);
    return next;
  }, []);

  const dismiss = useCallback(async () => {
    const next = await dismissUpdate();
    setStatus(next);
    return next;
  }, []);

  return { status, check, apply, dismiss };
}

export function isUpdateDismissedNow(status: UpdateStatus | null): boolean {
  if (!status?.dismissedUntil) return false;
  return status.dismissedUntil * 1000 > Date.now();
}
