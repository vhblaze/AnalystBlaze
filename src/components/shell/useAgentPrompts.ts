import { useCallback, useEffect, useRef, useState } from "react";
import type { useAuth } from "@/hooks/useAuth";
import type { useUpdater } from "@/hooks/useUpdater";
import { toast } from "@/hooks/use-toast";
import { useI18n } from "@/i18n";
import { splitBackendErrorCode } from "@/lib/backend-errors";
import {
  cancelDeviceReplacement,
  cancelDeviceTransfer,
  confirmDeviceTransfer,
  getActiveAnnouncements,
  getPrivilegedHelperStatus,
  getTodaysAutomaticActions,
  installPrivilegedHelper,
  isTauriRuntime,
  listenToAnnouncements,
  listenToRemoteCommandConfirmation,
  listenToShadowStorageNeedsConsent,
  replaceDevice,
  resolveRemoteCommandConfirmation,
  restartPrivilegedHelper,
  setShadowStorageConsent,
  type Announcement,
  type RemoteCommandConfirmationRequest,
} from "@/services/tauri/agent";
import type { RequestConfirmation } from "./useConfirmation";

type Auth = ReturnType<typeof useAuth>;
type Updater = ReturnType<typeof useUpdater>;

/**
 * Tudo o que o agente ou o servidor pede ao usuário sem ele ter clicado em
 * nada: confirmação de pedido remoto, anúncios, consentimento do Shadow
 * Storage, transferência de PC, resumo do que foi feito hoje, saúde do
 * componente de sistema e a escolha de PC no limite de dispositivos.
 * Saiu do AppShell sem mudança de comportamento (etapa 1 do doc 15).
 */
export function useAgentPrompts({
  auth,
  updater,
  requestConfirmation,
}: {
  auth: Auth;
  updater: Updater;
  requestConfirmation: RequestConfirmation;
}) {
  const { t } = useI18n();
  const [remoteConfirmationQueue, setRemoteConfirmationQueue] = useState<RemoteCommandConfirmationRequest[]>([]);
  const [announcements, setAnnouncements] = useState<Announcement[]>([]);
  // Fecha o popup so por esta sessao do app - o item no sininho continua, e o
  // aviso volta na proxima abertura enquanto o e-mail nao for verificado.
  const [emailNoticeDismissed, setEmailNoticeDismissed] = useState(false);
  const [dismissedAnnouncementIds, setDismissedAnnouncementIds] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("analystblaze.dismissedAnnouncements") ?? "[]");
    } catch {
      return [];
    }
  });
  const helperCheckRef = useRef({ running: false, lastAt: 0 });

  const handleUpdateNow = useCallback(() => {
    updater
      .apply()
      .catch((error) => {
        toast({
          title: "Atualização não foi instalada",
          description: String(error),
          variant: "destructive",
        });
      });
  }, [updater]);

  const handleUpdateLater = useCallback(() => {
    void updater.dismiss();
  }, [updater]);

  const deviceLimitChoices = auth.status?.device_limit_choices ?? [];
  const [replacingDevice, setReplacingDevice] = useState(false);
  const handleReplaceDevice = useCallback(
    async (hwId: string) => {
      setReplacingDevice(true);
      try {
        await replaceDevice(hwId);
      } catch (error) {
        await cancelDeviceReplacement().catch(() => undefined);
        toast({
          title: t("deviceLimit.failedTitle"),
          description: splitBackendErrorCode(String(error)).message,
          variant: "destructive",
        });
      } finally {
        setReplacingDevice(false);
        await auth.refreshStatus().catch(() => undefined);
      }
    },
    [auth, t],
  );
  const handleCancelDeviceLimit = useCallback(async () => {
    await cancelDeviceReplacement().catch(() => undefined);
    await auth.refreshStatus().catch(() => undefined);
  }, [auth]);

  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;

    const handleRemoteConfirmation = async (request: RemoteCommandConfirmationRequest) => {
      setRemoteConfirmationQueue((current) => [
        ...current.filter((item) => item.requestId !== request.requestId),
        request,
      ]);
      toast({
        title: "Pedido do dashboard recebido",
        description: request.title || request.actionName,
      });
      const approved = await requestConfirmation({
        title: request.title || request.actionName,
        description: request.description,
        risk: request.risk || "sensivel",
        snapshot: request.snapshot,
        remote: true,
        timeoutMs: 115_000,
      });
      setRemoteConfirmationQueue((current) =>
        current.filter((item) => item.requestId !== request.requestId),
      );
      if (!disposed) {
        await resolveRemoteCommandConfirmation(request.requestId, approved);
      }
    };

    void listenToRemoteCommandConfirmation((request) => {
      void handleRemoteConfirmation(request);
    }).then((dispose) => {
      cleanup = dispose;
      if (disposed) cleanup();
    });

    return () => {
      disposed = true;
      cleanup?.();
    };
  }, [requestConfirmation]);

  useEffect(() => {
    getActiveAnnouncements().then(setAnnouncements).catch(() => undefined);
    let dispose: (() => void) | undefined;
    listenToAnnouncements(setAnnouncements).then((unlisten) => {
      dispose = unlisten;
    });
    return () => dispose?.();
  }, []);

  // Shadow-copy storage hit its limit and the user has never chosen whether
  // AnalystBlaze may raise it for them. This is a preference to decide, not
  // an alert to acknowledge, so it surfaces as a two-choice card in Insights
  // rather than a modal that interrupts. The listener just flips a flag; the
  // card and its "handle it for me" / "I'll do it myself" buttons live in
  // the Insights view. Either choice is stored on the device so it never
  // asks again.
  const [shadowConsentNeeded, setShadowConsentNeeded] = useState(false);
  useEffect(() => {
    let disposed = false;
    let dispose: (() => void) | undefined;
    listenToShadowStorageNeedsConsent(() => {
      if (!disposed) setShadowConsentNeeded(true);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else dispose = unlisten;
    });
    return () => {
      disposed = true;
      dispose?.();
    };
  }, []);
  const resolveShadowConsent = useCallback(async (choice: "auto" | "manual") => {
    await setShadowStorageConsent(choice).catch(() => undefined);
    setShadowConsentNeeded(false);
  }, []);

  // The login succeeded but this PC is linked to another account. Moving it
  // is never automatic: only the person physically at this keyboard can
  // authorize it, because on a shared computer an automatic move would hand
  // the PC and its history to whoever signed in next. Declining drops the
  // login and leaves the PC where it is.
  useEffect(() => {
    if (!auth.status?.device_transfer_pending) return;
    let disposed = false;

    const ask = async () => {
      const approved = await requestConfirmation({
        title: t("deviceTransfer.title"),
        description: t("deviceTransfer.body"),
        risk: t("deviceTransfer.risk"),
        snapshot: false,
      });
      if (disposed) return;
      try {
        if (approved) {
          await confirmDeviceTransfer();
          toast({
            title: t("deviceTransfer.doneTitle"),
            description: t("deviceTransfer.doneBody"),
          });
        } else {
          await cancelDeviceTransfer();
        }
      } catch (error) {
        await cancelDeviceTransfer().catch(() => undefined);
        toast({
          title: t("deviceTransfer.failedTitle"),
          description: splitBackendErrorCode(String(error)).message,
          variant: "destructive",
        });
      }
      if (!disposed) await auth.refreshStatus().catch(() => undefined);
    };

    void ask();
    return () => {
      disposed = true;
    };
    // Intentionally keyed on the flag alone: re-running on every auth change
    // would reopen the prompt while it is still on screen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [auth.status?.device_transfer_pending]);

  // Once per local day, if AnalystBlaze did anything on its own today,
  // surface a plain summary of it - the "here's what was done on your PC"
  // end-of-day note. Dismissed-per-day via localStorage so it shows at
  // most once daily, whenever the app is next open.
  useEffect(() => {
    const todayKey = new Date().toISOString().slice(0, 10);
    const seenKey = `analystblaze.autoActionsSummarySeen.${todayKey}`;
    try {
      if (localStorage.getItem(seenKey)) return;
    } catch {
      // storage unavailable - fall through and just show it
    }
    let cancelled = false;
    getTodaysAutomaticActions()
      .then((actions) => {
        if (cancelled || actions.length === 0) return;
        toast({
          title: t("autoActionsSummary.title"),
          description: actions.map((action) => `• ${action.message}`).join("\n"),
        });
        try {
          localStorage.setItem(seenKey, "1");
        } catch {
          // non-critical
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [t]);

  const dismissAnnouncement = useCallback((id: string) => {
    setDismissedAnnouncementIds((current) => {
      const next = current.includes(id) ? current : [...current, id];
      try {
        localStorage.setItem("analystblaze.dismissedAnnouncements", JSON.stringify(next));
      } catch {
        // Non-critical preference persistence.
      }
      return next;
    });
  }, []);

  // The privileged helper is a local Windows service that runs elevated
  // admin actions (TEMP cleanup, network/DNS, scheduled defrag, frame
  // capture stop, ...). When it is not installed, stopped, or on a stale
  // version after an app update, those actions silently do nothing - which
  // is why a lot of installs run below their full capability without the
  // user knowing. This watches its health on a timer and on window focus,
  // and pops one dialog offering the right fix (install / start / sync /
  // reinstall). Declining snoozes for a few days rather than dismissing
  // forever; a different kind of problem re-prompts regardless of snooze;
  // once healthy the snooze is cleared.
  useEffect(() => {
    if (!auth.ready || !isTauriRuntime()) return;
    let disposed = false;

    const SNOOZE_KEY = "analystblaze.helper.health.v1";
    const SNOOZE_MS = 3 * 24 * 60 * 60 * 1000;
    const BLOCKED_SNOOZE_MS = 7 * 24 * 60 * 60 * 1000;
    const CHECK_INTERVAL_MS = 30 * 60 * 1000;
    const FOCUS_DEBOUNCE_MS = 5 * 60 * 1000;

    type Kind = "ok" | "not_installed" | "stopped" | "outdated" | "broken" | "blocked";

    const classify = (s: Awaited<ReturnType<typeof getPrivilegedHelperStatus>>): Kind => {
      if (s.available && !s.requiresUpdate) return "ok";
      if (!s.installed) return s.canRequestUac ? "not_installed" : "blocked";
      if (!s.running) return "stopped";
      if (s.requiresUpdate) return "outdated";
      return "broken";
    };

    const readSnooze = (): { until: number; kind: string } => {
      try {
        const raw = JSON.parse(window.localStorage.getItem(SNOOZE_KEY) ?? "{}");
        return { until: Number(raw.until) || 0, kind: String(raw.kind ?? "") };
      } catch {
        return { until: 0, kind: "" };
      }
    };
    const writeSnooze = (kind: Kind, ms: number) => {
      try {
        window.localStorage.setItem(SNOOZE_KEY, JSON.stringify({ until: Date.now() + ms, kind }));
      } catch {
        // Non-critical preference persistence.
      }
    };
    const clearSnooze = () => {
      try {
        window.localStorage.removeItem(SNOOZE_KEY);
      } catch {
        // Non-critical.
      }
    };

    const COPY: Record<
      Exclude<Kind, "ok">,
      { title: string; body: string; fix?: () => Promise<unknown> }
    > = {
      not_installed: {
        title: t("helperHealth.titleNotInstalled"),
        body: t("helperHealth.bodyNotInstalled"),
        fix: installPrivilegedHelper,
      },
      stopped: {
        title: t("helperHealth.titleStopped"),
        body: t("helperHealth.bodyStopped"),
        fix: restartPrivilegedHelper,
      },
      outdated: {
        title: t("helperHealth.titleOutdated"),
        body: t("helperHealth.bodyOutdated"),
        fix: restartPrivilegedHelper,
      },
      broken: {
        title: t("helperHealth.titleBroken"),
        body: t("helperHealth.bodyBroken"),
        fix: installPrivilegedHelper,
      },
      blocked: {
        // Nothing the app can do from here - the user has to reinstall
        // AnalystBlaze machine-wide. Informational only, snoozed longer.
        title: t("helperHealth.titleBlocked"),
        body: t("helperHealth.bodyBlocked"),
      },
    };

    const check = async () => {
      if (disposed || helperCheckRef.current.running) return;
      helperCheckRef.current.running = true;
      try {
        const status = await getPrivilegedHelperStatus();
        if (disposed) return;
        const kind = classify(status);
        if (kind === "ok") {
          clearSnooze();
          return;
        }

        const snooze = readSnooze();
        if (snooze.kind === kind && snooze.until > Date.now()) return;

        const copy = COPY[kind];
        const approved = await requestConfirmation({
          title: copy.title,
          description: copy.body,
          risk: t("helperHealth.risk"),
          snapshot: false,
        });
        if (disposed) return;

        if (!approved || !copy.fix) {
          writeSnooze(kind, kind === "blocked" ? BLOCKED_SNOOZE_MS : SNOOZE_MS);
          return;
        }

        toast({ title: t("helperHealth.workingTitle"), description: t("helperHealth.workingBody") });
        try {
          let next = await copy.fix();
          if (disposed) return;
          // sc.exe start returns while the service is still START_PENDING,
          // so a status() taken right after can still read "not running" -
          // give it a moment and re-probe once before deciding it failed.
          if (classify(next as Awaited<ReturnType<typeof getPrivilegedHelperStatus>>) !== "ok") {
            await new Promise((r) => setTimeout(r, 2500));
            if (disposed) return;
            next = await getPrivilegedHelperStatus();
          }
          if (classify(next as Awaited<ReturnType<typeof getPrivilegedHelperStatus>>) === "ok") {
            clearSnooze();
            toast({ title: t("helperHealth.doneTitle"), description: t("helperHealth.doneBody") });
          } else {
            writeSnooze(kind, SNOOZE_MS);
            toast({
              title: t("helperHealth.stillDownTitle"),
              description: t("helperHealth.stillDownBody"),
              variant: "destructive",
            });
          }
        } catch (error) {
          if (disposed) return;
          writeSnooze(kind, SNOOZE_MS);
          toast({
            title: t("helperHealth.failedTitle"),
            description: String(error),
            variant: "destructive",
          });
        }
      } catch {
        // Status probe failed (helper query hiccup) - try again next tick.
      } finally {
        helperCheckRef.current.running = false;
        helperCheckRef.current.lastAt = Date.now();
      }
    };

    void check();
    const interval = window.setInterval(() => void check(), CHECK_INTERVAL_MS);
    const onFocus = () => {
      if (Date.now() - helperCheckRef.current.lastAt >= FOCUS_DEBOUNCE_MS) void check();
    };
    window.addEventListener("focus", onFocus);

    return () => {
      disposed = true;
      window.clearInterval(interval);
      window.removeEventListener("focus", onFocus);
    };
  }, [auth.ready, requestConfirmation, t]);

  return {
    remoteConfirmationQueue,
    announcements,
    dismissedAnnouncementIds,
    dismissAnnouncement,
    emailNoticeDismissed,
    setEmailNoticeDismissed,
    shadowConsentNeeded,
    resolveShadowConsent,
    deviceLimitChoices,
    replacingDevice,
    handleReplaceDevice,
    handleCancelDeviceLimit,
    handleUpdateNow,
    handleUpdateLater,
  };
}
