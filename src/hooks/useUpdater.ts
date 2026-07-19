import { useCallback, useEffect, useState } from "react";
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

  useEffect(() => {
    let active = true;
    let dispose: (() => void) | undefined;

    getUpdateStatus()
      .then((next) => {
        if (active) setStatus(next);
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
