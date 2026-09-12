import { X } from "lucide-react";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "./Sidebar";
import { TopBar, type TopBarNotification, type TopBarSearchItem } from "./TopBar";
import { LoggedOutNotice } from "./LoggedOutNotice";
import { UpdateNotice } from "./UpdateNotice";
import { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { canUseAutomaticGameMode, useAuth } from "@/hooks/useAuth";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { isUpdateDismissedNow, useUpdater } from "@/hooks/useUpdater";
import { useI18n } from "@/i18n";
import {
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
  resolveRemoteCommandConfirmation,
  restartPrivilegedHelper,
  setShadowStorageConsent,
  type Announcement,
  type RemoteCommandConfirmationRequest,
} from "@/services/tauri/agent";
import type { DiskNearFullInfo } from "@/services/insights";

export type ViewKey = "dashboard" | "telemetry" | "insights" | "controls" | "disk" | "network" | "settings";

const Dashboard = lazy(() => import("./views/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("./views/Telemetry").then((module) => ({ default: module.Telemetry })));
const Insights = lazy(() => import("./views/Insights").then((module) => ({ default: module.Insights })));
const LocalControls = lazy(() => import("./views/LocalControls").then((module) => ({ default: module.LocalControls })));
const DiskExplorer = lazy(() => import("./views/DiskExplorer").then((module) => ({ default: module.DiskExplorer })));
const Network = lazy(() => import("./views/Network").then((module) => ({ default: module.Network })));
const Settings = lazy(() => import("./views/Settings").then((module) => ({ default: module.Settings })));

// The backend prefixes a handful of auth/hardware errors with a stable,
// language-independent code (e.g. "DEVICE_LIMIT_REACHED::<translated
// message>") specifically so the UI can strip it for display and react to it
// (offer a way to fix it) without pattern-matching on prose that changes per
// locale. Errors without a known prefix pass through untouched.
const BACKEND_ERROR_CODES = [
  "HARDWARE_INACTIVE",
  "DEVICE_LIMIT_REACHED",
  "HARDWARE_ALREADY_LINKED",
  "DEVICE_TRANSFER_COOLDOWN",
] as const;

// No regex: the message half is free-form translated prose that can contain
// newlines and regex metacharacters, and building a pattern from these codes
// invites escaping bugs for no benefit over a plain prefix scan.
export function splitBackendErrorCode(raw: string): { code: string | null; message: string } {
  for (const code of BACKEND_ERROR_CODES) {
    const marker = code + "::";
    const at = raw.indexOf(marker);
    if (at !== -1) return { code, message: raw.slice(at + marker.length).trim() };
  }
  return { code: null, message: raw };
}

export function AppShell() {
  const [view, setView] = useState<ViewKey>("dashboard");
  const [focusDiskUsage, setFocusDiskUsage] = useState(false);
  const [focusTracerouteTarget, setFocusTracerouteTarget] = useState<string | null>(null);
  const [diskNearFullInfo, setDiskNearFullInfo] = useState<DiskNearFullInfo | null>(null);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
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
  const auth = useAuth();
  const telemetry = useAgentTelemetry();
  const updater = useUpdater();
  const { t } = useI18n();
  const track = useTelemetry("navigation");

  const handleUpdateNow = useCallback(() => {
    updater
      .apply()
      .catch((error) => {
        toast({
          title: "Atualizacao nao foi instalada",
          description: String(error),
          variant: "destructive",
        });
      });
  }, [updater]);

  const handleUpdateLater = useCallback(() => {
    void updater.dismiss();
  }, [updater]);

  const loggedOutError = useMemo(() => {
    if (auth.message.key !== "agent.messages.error") return null;
    return splitBackendErrorCode(t(auth.message.key, auth.message.params));
  }, [auth.message, t]);

  const titles = useMemo<Record<ViewKey, string>>(
    () => ({
      dashboard: t("nav.dashboard"),
      telemetry: t("nav.telemetry"),
      insights: t("nav.insights"),
      controls: t("nav.controls"),
      disk: t("nav.disk"),
      network: t("nav.network"),
      settings: t("nav.settings"),
    }),
    [t],
  );

  const handleViewChange = useCallback(
    (nextView: ViewKey) => {
      setView(nextView);
      track("navigation_change", { view: nextView });
    },
    [track],
  );

  const openDiskUsageDetails = useCallback(() => {
    setFocusDiskUsage(true);
    handleViewChange("disk");
  }, [handleViewChange]);

  const openNetworkDetails = useCallback(
    (autoTracerouteTarget?: string) => {
      if (autoTracerouteTarget) setFocusTracerouteTarget(autoTracerouteTarget);
      handleViewChange("network");
    },
    [handleViewChange],
  );

  const requestConfirmation = useCallback((request: Omit<ConfirmRequest, "resolve" | "id">) => {
    return new Promise<boolean>((resolve) => {
      const id = crypto.randomUUID();
      let settled = false;
      const timeoutId = request.timeoutMs
        ? window.setTimeout(() => {
            if (settled) return;
            settled = true;
            setConfirmRequest((current) => (current?.id === id ? null : current));
            resolve(false);
          }, request.timeoutMs)
        : undefined;

      const nextRequest: ConfirmRequest = {
        ...request,
        id,
        resolve: (approved) => {
          if (settled) return;
          settled = true;
          if (timeoutId) window.clearTimeout(timeoutId);
          resolve(approved);
        },
      };
      setConfirmRequest((current) => {
        current?.resolve(false);
        return nextRequest;
      });
    });
  }, []);

  const closeConfirmation = useCallback((approved: boolean) => {
    setConfirmRequest((current) => {
      current?.resolve(approved);
      return null;
    });
  }, []);

  const runConfirmed = useCallback(
    async (request: Omit<ConfirmRequest, "id" | "resolve">, action: () => Promise<unknown>) => {
      const approved = await requestConfirmation(request);
      if (!approved) return false;
      await action();
      return true;
    },
    [requestConfirmation],
  );

  // Starter plan gets a real 1h/week Game Mode budget now (enforced
  // server-side, see activate_game_mode in lib.rs) instead of an outright
  // block - only redirect to billing once that budget is actually
  // exhausted, not pre-emptively.
  const activateGameModeWithUpsell = useCallback(async () => {
    const result = await auth.activateGameMode();
    if (result?.blockedReason === "weekly_limit_reached") {
      await auth.openBilling();
    }
  }, [auth]);

  // "I'll do it myself" from an Insights card - runs the same action right
  // now with the same confirmation dialog its own dedicated button uses,
  // rather than a separate/different flow just because it started from a
  // recommendation instead of a button.
  const applyInsightActionLocally = useCallback(
    (actionName: string) => {
      if (actionName === "APPLY_GAME_MODE") {
        return runConfirmed(
          {
            title: "Ativar Modo Gamer",
            description: "O agente aplica jogo em alta prioridade, reduz fundo, ajusta energia/visual, faz limpeza segura reversivel e mede rede. A restauracao fica pronta para quando voce sair do jogo.",
            risk: "sensivel",
            snapshot: true,
          },
          activateGameModeWithUpsell,
        );
      }
      if (actionName === "EMPTY_TEMP") {
        return runConfirmed(
          {
            title: "Limpeza profunda TEMP",
            description: "Move arquivos temporarios destravados com pelo menos 5 minutos para quarentena e tenta incluir a TEMP do Windows quando o helper permitir.",
            risk: "sensivel",
            snapshot: true,
          },
          auth.cleanTempDeep,
        );
      }
      if (actionName === "ENABLE_SCHEDULED_DEFRAG") {
        return runConfirmed(
          {
            title: "Reativar otimizacao automatica de disco",
            description: "Reativa a tarefa nativa do Windows que desfragmenta seu HD periodicamente, mantendo a leitura de arquivos rapida com o tempo. Exige o helper privilegiado instalado.",
            risk: "sensivel",
            snapshot: false,
          },
          auth.enableScheduledDefrag,
        );
      }
      return Promise.reject(new Error(`Acao nao suportada localmente: ${actionName}`));
    },
    [runConfirmed, activateGameModeWithUpsell, auth],
  );

  const notificationItems = useMemo<TopBarNotification[]>(
    () => {
      const items: TopBarNotification[] = remoteConfirmationQueue.map((request) => ({
        id: request.requestId,
        title: request.title || request.actionName,
        description: request.description,
        tone: "warning",
      }));
      if (!auth.user?.emailVerified) {
        const days = auth.user?.emailVerificationDaysRemaining ?? null;
        items.unshift({
          id: "email-verification",
          title: t("emailVerification.bellTitle"),
          description:
            typeof days === "number"
              ? t("emailVerification.bellDescriptionWithDays", { days })
              : t("emailVerification.bellDescription"),
          tone: typeof days === "number" && days <= 7 ? "danger" : "warning",
        });
      }
      const update = updater.status;
      if (update?.available && (update.mandatory || !isUpdateDismissedNow(update))) {
        items.unshift({
          id: "update-available",
          title: update.mandatory
            ? t("update.availableTitleMandatory", { version: update.version ?? "" })
            : t("update.availableTitle", { version: update.version ?? "" }),
          description: update.notes?.trim() || t("update.notesFallback"),
          tone: update.mandatory ? "danger" : "info",
        });
      }
      announcements
        .filter((announcement) => !dismissedAnnouncementIds.includes(announcement.id))
        .forEach((announcement) => {
          items.push({
            id: `announcement:${announcement.id}`,
            title: announcement.title,
            description: announcement.body,
            tone: announcement.tone,
            createdAt: announcement.createdAt,
          });
        });
      return items;
    },
    [remoteConfirmationQueue, updater.status, announcements, dismissedAnnouncementIds, auth.user, t],
  );

  const searchItems = useMemo<TopBarSearchItem[]>(
    () => [
      {
        id: "view-dashboard",
        title: t("nav.dashboard"),
        description: "Visao geral do agente, saude e atalhos principais.",
        keywords: ["home", "inicio", "painel"],
        hint: "view",
        onSelect: () => handleViewChange("dashboard"),
      },
      {
        id: "view-telemetry",
        title: t("nav.telemetry"),
        description: "Amostras locais de CPU, GPU, RAM, disco, rede e janela ativa.",
        keywords: ["metricas", "tempo real", "cpu", "gpu", "ram"],
        hint: "view",
        onSelect: () => handleViewChange("telemetry"),
      },
      {
        id: "view-insights",
        title: t("nav.insights"),
        description: "Insights da IA baseados na telemetria do backend.",
        keywords: ["ia", "recomendacoes", "insights"],
        hint: "view",
        onSelect: () => handleViewChange("insights"),
      },
      {
        id: "view-controls",
        title: t("nav.controls"),
        description: "Helper admin, snapshots, servicos, energia e acoes reversiveis.",
        keywords: ["helper", "windows", "servicos", "temp", "energia"],
        hint: "view",
        onSelect: () => handleViewChange("controls"),
      },
      {
        id: "view-settings",
        title: t("nav.settings"),
        description: "Conta, login, idioma e configuracoes do agente.",
        keywords: ["conta", "idioma", "login"],
        hint: "view",
        onSelect: () => handleViewChange("settings"),
      },
      {
        id: "action-start-agent",
        title: "Iniciar agente",
        description: "Conecta o agente local e inicia a telemetria.",
        keywords: ["start", "telemetria", "conectar"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () => void auth.start(),
      },
      {
        id: "action-collect-sample",
        title: "Coletar amostra agora",
        description: "Atualiza a leitura local de telemetria uma vez.",
        keywords: ["sample", "amostra", "metricas"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () => void auth.collectSample(),
      },
      {
        id: "action-pc-clean-fast",
        title: "Aplicar PC limpo/rapido",
        description: "Executa baseline, limpeza segura, visual de desempenho, apps de fundo e score medido.",
        keywords: ["performance", "limpo", "rapido", "score"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Aplicar PC limpo/rapido",
              description: "O agente mede antes/depois, aplica somente acoes allowlisted com snapshots locais e mostra o ganho real deste computador.",
              risk: "sensivel",
              snapshot: true,
            },
            auth.pcCleanFast,
          ),
      },
      {
        id: "action-game-mode",
        title: "Ativar Modo Gamer",
        description: "Aplica perfil completo: jogo em alta prioridade, fundo reduzido, limpeza segura, energia, visual e rede.",
        keywords: ["jogo", "game", "prioridade", "otimizacao"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Ativar Modo Gamer",
              description: "O agente aplica um pacote seguro: prioriza jogo/app ativo, reduz apps de fundo, usa limpeza TEMP reversivel, ajusta energia/visual e mede rede. Deep clean e purge nao entram neste fluxo.",
              risk: "sensivel",
              snapshot: true,
            },
            auth.activateGamePerformanceMode,
          ),
      },
      {
        id: "action-restore-game-mode",
        title: "Restaurar Modo Gamer",
        description: "Restaura a sessao ativa do Modo Gamer e seus snapshots.",
        keywords: ["restaurar", "restore", "jogo"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Restaurar Modo Gamer",
              description: "Restaura snapshots da sessao ativa de Modo Gamer, incluindo plano de energia e prioridades de processos quando disponiveis.",
              risk: "seguro",
              snapshot: false,
            },
            auth.restoreGameMode,
          ),
      },
      {
        id: "action-restore-snapshots",
        title: "Restaurar snapshots",
        description: "Desfaz alteracoes pendentes salvas pelo agente local.",
        keywords: ["rollback", "restore", "snapshot"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Restaurar snapshots",
              description: "O agente vai tentar desfazer alteracoes pendentes de energia, limpeza, apps de inicializacao e servicos.",
              risk: "seguro",
              snapshot: false,
            },
            auth.restoreOptimizations,
          ),
      },
      {
        id: "action-deep-temp",
        title: "Limpeza profunda TEMP",
        description: "Move arquivos TEMP destravados com pelo menos 5 minutos para quarentena.",
        keywords: ["temp", "limpeza", "cleanup"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Limpeza profunda TEMP",
              description: "Move arquivos temporarios destravados com pelo menos 5 minutos para quarentena e tenta incluir a TEMP do Windows quando o helper permitir.",
              risk: "sensivel",
              snapshot: true,
            },
            auth.cleanTempDeep,
          ),
      },
      {
        id: "action-purge-cleanup",
        title: "Purgar quarentena",
        description: "Apaga permanentemente arquivos em quarentena para liberar espaco real.",
        keywords: ["purge", "quarentena", "espaco", "disco"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: "Purgar quarentena",
              description: "Apaga permanentemente a quarentena de limpeza. Depois disso esses arquivos nao poderao ser restaurados.",
              risk: "sensivel",
              snapshot: false,
            },
            auth.purgeCleanup,
          ),
      },
      {
        id: "action-login",
        title: "Fazer login pela Web",
        description: "Abre o pareamento/login do agente desktop.",
        keywords: ["login", "conta", "pareamento"],
        hint: "auth",
        disabled: auth.busy || Boolean(auth.status?.authenticated),
        onSelect: () => void auth.login(),
      },
    ],
    [auth, handleViewChange, runConfirmed, t],
  );

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

  return (
    <div className="relative flex h-screen w-full overflow-hidden text-slate-100">
      <div className="pointer-events-none absolute inset-0 grid-bg opacity-60" />
      <div className="pointer-events-none absolute inset-0 scanline opacity-40" />
      <div className="pointer-events-none absolute inset-0 noise opacity-[0.035] mix-blend-overlay" />

      <Sidebar
        view={view}
        onChange={handleViewChange}
        user={auth.user}
        status={auth.status}
        busy={auth.busy}
        onLogin={auth.login}
        onLogout={auth.logout}
      />

      <main className="relative z-10 flex flex-1 flex-col overflow-hidden">
        <TopBar
          title={titles[view]}
          user={auth.user}
          status={auth.status}
          pendingNotifications={notificationItems.length}
          notifications={notificationItems}
          searchItems={searchItems}
          onNotificationsClick={() => {
            track("notifications_clicked", { pending: notificationItems.length });
          }}
          onNotificationClick={(id) => {
            if (id === "update-available") {
              handleUpdateNow();
              return;
            }
            if (id.startsWith("announcement:")) {
              dismissAnnouncement(id.slice("announcement:".length));
              return;
            }
            setView("dashboard");
          }}
        />
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto max-w-6xl px-5 py-6 sm:px-8 lg:px-10 lg:py-8">
            {remoteConfirmationQueue.length > 0 && (
              <RemoteConfirmationNotice
                request={remoteConfirmationQueue[remoteConfirmationQueue.length - 1]}
                count={remoteConfirmationQueue.length}
              />
            )}
            {!auth.ready ? (
              <div className="text-sm text-slate-500">{t("app.loading")}</div>
            ) : view === "dashboard" ? (
              <Suspense fallback={<ViewFallback />}>
                <Dashboard
                  user={auth.user}
                  status={auth.status}
                  telemetry={telemetry}
                  networkActionPending={auth.networkActionPending}
                  onStartAgent={auth.start}
                  onActivateGameMode={async () => {
                    await runConfirmed(
                      {
                        title: "Ativar Modo Gamer",
                        description: "O agente aplica jogo em alta prioridade, reduz fundo, ajusta energia/visual, faz limpeza segura reversivel e mede rede. A restauracao fica pronta para quando voce sair do jogo.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      activateGameModeWithUpsell,
                    );
                  }}
                  onRestoreGameMode={async () => {
                    await runConfirmed(
                      {
                        title: "Desativar Modo Gamer",
                        description: "Restaura snapshots da sessao ativa de Modo Gamer, incluindo plano de energia e prioridades de processos quando disponiveis.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreGameMode,
                    );
                  }}
                  onApplyPcCleanFast={async () => {
                    await runConfirmed(
                      {
                        title: "Aplicar PC limpo/rapido",
                        description: "Executa Performance Scan, limpeza segura, ajuste visual, priorizacao de apps de fundo e Modo Gamer se houver jogo detectado.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      auth.pcCleanFast,
                    );
                  }}
                  onOpenDiskUsage={openDiskUsageDetails}
                  onOpenNetwork={openNetworkDetails}
                  busy={auth.busy}
                />
              </Suspense>
            ) : view === "telemetry" ? (
              <Suspense fallback={<ViewFallback />}>
                <Telemetry
                  latestSample={auth.sample ?? telemetry}
                  agentMode={auth.status?.mode ?? telemetry?.telemetry_mode}
                  isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                  busy={auth.busy}
                  onCollectSample={auth.collectSample}
                  onSetTelemetryMode={auth.setTelemetryMode}
                  onOpenDiskUsage={openDiskUsageDetails}
                  onOpenNetwork={openNetworkDetails}
                />
              </Suspense>
            ) : view === "insights" ? (
              <Suspense fallback={<ViewFallback />}>
                <Insights
                  telemetry={telemetry}
                  diskNearFullInfo={diskNearFullInfo}
                  onOpenDiskUsage={openDiskUsageDetails}
                  onOpenNetwork={openNetworkDetails}
                  onApplyInsightActionLocally={applyInsightActionLocally}
                  onRequestAgentApplyInsight={auth.requestAgentApplyInsight}
                  shadowConsentNeeded={shadowConsentNeeded}
                  onResolveShadowConsent={resolveShadowConsent}
                />
              </Suspense>
            ) : view === "controls" ? (
              <Suspense fallback={<ViewFallback />}>
                <LocalControls
                  status={auth.status}
                  automaticGameModeAllowed={canUseAutomaticGameMode(auth.status)}
                  busy={auth.busy}
                  onActivateGameMode={() =>
                    runConfirmed(
                      {
                        title: "Ativar Modo Gamer",
                        description: "Aplica o perfil completo de jogo: prioridade alta para o alvo, fundo reduzido, limpeza segura, energia/visual e rede.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      activateGameModeWithUpsell,
                    )
                  }
                  onRestoreOptimizations={() =>
                    runConfirmed(
                      {
                        title: "Restaurar snapshots",
                        description: "O agente vai tentar desfazer alteracoes pendentes de energia, limpeza, apps de inicializacao e servicos.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreOptimizations,
                    )
                  }
                  onDisableStartup={(name, location) =>
                    runConfirmed(
                      {
                        title: `Desativar ${name}`,
                        description: `Remove o app da inicializacao do Windows em ${location ?? "registro detectado"} e cria snapshot para restaurar depois.`,
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.disableStartup(name, location),
                    )
                  }
                  onRestoreStartup={(name) =>
                    runConfirmed(
                      {
                        title: name ? `Restaurar ${name}` : "Restaurar apps de inicializacao",
                        description: "Restaura o valor original salvo no snapshot local do Registro.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restoreStartup(name),
                    )
                  }
                  onStopService={(name) =>
                    runConfirmed(
                      {
                        title: `Parar servico ${name}`,
                        description: "O servico sera parado apenas se passar pela denylist local. Um snapshot guarda se ele estava rodando antes.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.stopService(name),
                    )
                  }
                  onRestoreService={(name) =>
                    runConfirmed(
                      {
                        title: name ? `Restaurar servico ${name}` : "Restaurar servicos",
                        description: "Tenta religar apenas servicos que estavam rodando antes da acao.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restoreService(name),
                    )
                  }
                  onSetPowerPlan={(plan) =>
                    runConfirmed(
                      {
                        title: plan === "high_performance" ? "Ativar alto desempenho" : plan === "power_saver" ? "Ativar economia de energia" : "Ativar plano equilibrado",
                        description: "Altera o plano de energia do Windows e cria snapshot local para restaurar o plano anterior depois.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.setPowerPlan(plan),
                    )
                  }
                  onApplyVisualPerformance={() =>
                    runConfirmed(
                      {
                        title: "Ativar visual de desempenho",
                        description: "Reduz animacoes, transparencia e efeitos visuais do Windows para deixar o sistema mais leve (reversivel por snapshot). Pode causar um pequeno engasgo de um instante em outros programas abertos, principalmente navegadores - e o Windows avisando todo mundo da mudanca, um comportamento normal do sistema, nao um erro.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      auth.applyVisualPerformance,
                    )
                  }
                  onRestoreVisualPerformance={() =>
                    runConfirmed(
                      {
                        title: "Restaurar efeitos visuais",
                        description: "Restaura os valores visuais do Windows salvos no snapshot local mais recente.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreVisualPerformance,
                    )
                  }
                  onDeepCleanTemp={() =>
                    runConfirmed(
                      {
                        title: "Limpeza profunda TEMP",
                        description: "Move arquivos temporarios destravados com pelo menos 5 minutos para quarentena e tenta incluir a TEMP do Windows quando o helper permitir.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      auth.cleanTempDeep,
                    )
                  }
                  onPurgeCleanup={() =>
                    runConfirmed(
                      {
                        title: "Purgar quarentena",
                        description: "Apaga permanentemente a quarentena de limpeza. Depois disso esses arquivos nao poderao ser restaurados.",
                        risk: "sensivel",
                        snapshot: false,
                      },
                      auth.purgeCleanup,
                    )
                  }
                  onRestoreGameMode={() =>
                    runConfirmed(
                      {
                        title: "Restaurar Modo Gamer",
                        description: "Restaura snapshots da sessao ativa de Modo Gamer, incluindo plano de energia e prioridades de processos quando disponiveis.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreGameMode,
                    )
                  }
                  onApplyPcCleanFast={() =>
                    runConfirmed(
                      {
                        title: "Aplicar PC limpo/rapido",
                        description: "Executa Performance Scan, limpeza segura, ajuste visual, priorizacao de apps de fundo e Modo Gamer se houver jogo detectado.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      auth.pcCleanFast,
                    )
                  }
                  onRestorePerformanceSession={(sessionId) =>
                    runConfirmed(
                      {
                        title: "Restaurar Performance Suite",
                        description: "Restaura snapshots criados pelo perfil PC limpo/rapido, incluindo visual, prioridades e inicializacao atrasada quando existirem.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restorePerformance(sessionId),
                    )
                  }
                  onApplyCleanupCategory={(category, mode) =>
                    runConfirmed(
                      {
                        title: category === "cleanup_quarantine" ? "Purgar quarentena" : "Aplicar limpeza",
                        description: category === "cleanup_quarantine"
                          ? "Apaga permanentemente a quarentena para liberar espaco real."
                          : "Move arquivos elegiveis desta categoria para quarentena reversivel.",
                        risk: category === "cleanup_quarantine" ? "sensivel" : "seguro",
                        snapshot: category !== "cleanup_quarantine",
                      },
                      () => auth.applyCleanupCategory(category, mode),
                    )
                  }
                  onDelayStartupApp={(name, location) =>
                    runConfirmed(
                      {
                        title: `Atrasar ${name}`,
                        description: `Remove temporariamente ${name} da inicializacao direta e coloca na fila local do AnalystBlaze para iniciar depois.`,
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.delayStartup(name, location),
                    )
                  }
                  onRestoreDelayedStartupApp={(name) =>
                    runConfirmed(
                      {
                        title: name ? `Restaurar ${name}` : "Restaurar inicializacao atrasada",
                        description: "Restaura o valor de inicializacao salvo em snapshot local.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restoreDelayedStartup(name),
                    )
                  }
                />
              </Suspense>
            ) : view === "disk" ? (
              <Suspense fallback={<ViewFallback />}>
                <DiskExplorer
                  autoScan={focusDiskUsage}
                  onAutoScanHandled={() => setFocusDiskUsage(false)}
                  onDiskNearFullDetected={setDiskNearFullInfo}
                />
              </Suspense>
            ) : view === "network" ? (
              <Suspense fallback={<ViewFallback />}>
                <Network
                  busy={auth.busy}
                  isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                  autoTracerouteTarget={focusTracerouteTarget}
                  onAutoTracerouteHandled={() => setFocusTracerouteTarget(null)}
                  onFlushDnsCache={() =>
                    runConfirmed(
                      {
                        title: "Limpar cache de DNS",
                        description: "Executa ipconfig /flushdns. Nao exige admin e nao altera nenhum estado que precise de restauracao.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.flushDns,
                    )
                  }
                  onSetDnsServers={(adapterName, dnsServers) =>
                    runConfirmed(
                      {
                        title: "Alterar DNS do adaptador",
                        description: "Troca os servidores DNS do adaptador selecionado. O agente salva a configuracao atual em snapshot local para restaurar depois.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.setDnsServers(adapterName, dnsServers),
                    )
                  }
                  onResetWinsockCatalog={() =>
                    runConfirmed(
                      {
                        title: "Resetar catalogo Winsock",
                        description: "Executa netsh winsock reset. E disruptivo, exige reinicializacao do computador e nao pode ser desfeito automaticamente pelo agente.",
                        risk: "sensivel",
                        snapshot: false,
                      },
                      auth.resetWinsock,
                    )
                  }
                  onSetAdapterEnabled={async (adapterName, enabled, context) => {
                    if (context?.riskWarning) {
                      const acknowledged = await requestConfirmation({
                        title: "Atencao: essa interface parece estar em uso",
                        description: context.riskWarning,
                        risk: "sensivel",
                        snapshot: false,
                      });
                      if (!acknowledged) return false;
                    }
                    const baseDescription = enabled
                      ? "Reativa o adaptador de rede selecionado."
                      : "Desativa o adaptador de rede selecionado para forcar o trafego a sair por outro adaptador. O agente salva o estado atual em snapshot local para restaurar depois.";
                    return runConfirmed(
                      {
                        title: enabled ? "Ativar adaptador de rede" : "Desativar adaptador de rede",
                        description: context?.reason ? `${context.reason}\n\n${baseDescription}` : baseDescription,
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.setAdapterEnabled(adapterName, enabled),
                    );
                  }}
                  onApplyNetworkTune={(request) =>
                    runConfirmed(
                      {
                        title: "Aplicar otimizacao de TCP",
                        description: "Ajusta parametros da pilha TCP do Windows (auto-tuning, ECN, congestionamento ou registro por adaptador). O agente salva a configuracao atual em snapshot local; se ninguem confirmar, e revertido automaticamente.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.applyNetworkTune(request),
                    )
                  }
                  onRestartWindows={() =>
                    runConfirmed(
                      {
                        title: "Reiniciar o computador",
                        description: "Fecha todos os programas abertos e reinicia o Windows em alguns segundos. Necessario para aplicar ajustes de registro na rede.",
                        risk: "sensivel",
                        snapshot: false,
                      },
                      auth.restartWindowsNow,
                    )
                  }
                />
              </Suspense>
            ) : (
              <Suspense fallback={<ViewFallback />}>
                <Settings
                  user={auth.user}
                  status={auth.status}
                  syncingPlan={auth.syncingPlan}
                  onLogin={auth.login}
                  onLogout={auth.logout}
                  onOpenAccountSettings={auth.openAccountSettings}
                  onOpenBilling={auth.openBilling}
                  onSyncPlan={auth.syncPlan}
                  onOpenHistory={() => handleViewChange("controls")}
                />
              </Suspense>
            )}
          </div>
        </div>
      </main>
      {confirmRequest && (
        <ConfirmationDialog
          request={confirmRequest}
          onCancel={() => closeConfirmation(false)}
          onConfirm={() => closeConfirmation(true)}
        />
      )}
      {auth.user && !auth.user.emailVerified && !emailNoticeDismissed && (
        <EmailVerificationNotice
          daysRemaining={auth.user.emailVerificationDaysRemaining}
          onVerify={() => {
            setEmailNoticeDismissed(true);
            void auth.openAccountSettings();
          }}
          onLater={() => setEmailNoticeDismissed(true)}
        />
      )}
      <LoggedOutNotice
        visible={auth.ready && !auth.status?.authenticated}
        busy={auth.busy}
        onLogin={() => void auth.login()}
        errorMessage={loggedOutError?.message ?? null}
        showManageDevices={loggedOutError?.code === "DEVICE_LIMIT_REACHED"}
        onManageDevices={() => void auth.openAccountSettings()}
      />
      {/* Suppressed while logged out - stacking it with LoggedOutNotice would
          overlap two full-screen dialogs, and there's nothing to update to
          right now anyway without a session. */}
      {(!auth.ready || auth.status?.authenticated) && (
        <UpdateNotice
          status={updater.status}
          busy={auth.busy}
          onUpdateNow={handleUpdateNow}
          onLater={handleUpdateLater}
        />
      )}
    </div>
  );
}

type ConfirmRequest = {
  id: string;
  title: string;
  description: string;
  risk: string;
  snapshot: boolean;
  remote?: boolean;
  timeoutMs?: number;
  resolve: (approved: boolean) => void;
};

function RemoteConfirmationNotice({
  request,
  count,
}: {
  request: RemoteCommandConfirmationRequest;
  count: number;
}) {
  return (
    <div className="mb-4 rounded-xl border border-amber-300/30 bg-amber-400/10 px-4 py-3 text-sm text-amber-50 shadow-[0_18px_50px_-32px_hsl(45_100%_60%/0.6)]">
      <div className="font-mono text-[10px] uppercase tracking-[0.24em] text-amber-200/80">
        confirmacao pendente{count > 1 ? ` (${count})` : ""}
      </div>
      <div className="mt-1 font-semibold text-slate-50">{request.title || request.actionName}</div>
      <p className="mt-1 max-w-3xl text-xs leading-relaxed text-amber-100/80">
        O dashboard pediu permissao para aplicar esta acao neste computador. A janela principal foi trazida para frente; confirme ou recuse no pop-up local.
      </p>
    </div>
  );
}

/**
 * Aviso de e-mail nao verificado. A verificacao em si (pedir o codigo e
 * digitar os 6 digitos) acontece no site: os endpoints sao publicos e a conta
 * pode ate estar desativada, entao o navegador e o caminho que funciona nos
 * dois casos - aqui o app so avisa e leva pra la.
 *
 * "Agora nao" fecha por esta sessao do app, nao dispensa o aviso: o item no
 * sininho continua la, e o popup volta na proxima abertura enquanto o e-mail
 * nao for verificado. Esconder isso de vez seria esconder a desativacao.
 */
function EmailVerificationNotice({
  daysRemaining,
  onVerify,
  onLater,
}: {
  daysRemaining: number | null;
  onVerify: () => void;
  onLater: () => void;
}) {
  const { t } = useI18n();
  const isUrgent = typeof daysRemaining === "number" && daysRemaining <= 7;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("emailVerification.popupTitle")}
        className={`w-full max-w-lg rounded-2xl border bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)] ${
          isUrgent ? "border-rose-400/40" : "border-amber-400/30"
        }`}
      >
        <div
          className={`font-mono text-[10px] uppercase tracking-[0.25em] ${
            isUrgent ? "text-rose-300" : "text-amber-300"
          }`}
        >
          {t("emailVerification.bellTitle")}
        </div>
        <h2 className="mt-2 text-xl font-semibold text-slate-50">{t("emailVerification.popupTitle")}</h2>
        <p className="mt-2 text-sm leading-relaxed text-slate-400">
          {typeof daysRemaining === "number"
            ? t("emailVerification.popupBodyWithDays", { days: daysRemaining })
            : t("emailVerification.popupBody")}
        </p>
        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onLater}
            className="rounded-xl border border-slate-600/60 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-400/70"
          >
            {t("emailVerification.laterAction")}
          </button>
          <button
            onClick={onVerify}
            className="rounded-xl border border-cyan-400/50 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15"
          >
            {t("emailVerification.verifyAction")}
          </button>
        </div>
      </section>
    </div>
  );
}

function ConfirmationDialog({
  request,
  onCancel,
  onConfirm,
}: {
  request: ConfirmRequest;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 py-8 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={request.title}
        // Same fix as UpdateNotice: a long description (helper-health,
        // device-transfer, ...) used to push "Cancelar"/"Confirmar" off the
        // bottom of the screen with nothing to scroll and nothing to close -
        // this dialog fires far more often than the update one (every 30min
        // helper check, on every window focus), so it was the bigger source
        // of users getting stuck. Body scrolls on its own; the button row
        // stays outside that scroll region so it's never something you have
        // to scroll past a wall of text to reach.
        className="relative flex max-h-[85vh] w-full max-w-lg flex-col rounded-2xl border border-cyan-400/20 bg-slate-950 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)]"
      >
        <button
          type="button"
          onClick={onCancel}
          aria-label="Fechar"
          className="absolute right-3 top-3 z-10 rounded-lg p-1.5 text-slate-400 transition hover:bg-white/5 hover:text-slate-100"
        >
          <X className="h-4 w-4" />
        </button>

        <div className="min-h-0 flex-1 overflow-y-auto p-6 pb-4 pr-10">
          <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-amber-300">
            {request.remote ? "pedido recebido da web" : "confirmacao local"}
          </div>
          <h2 className="mt-2 text-xl font-semibold text-slate-50">{request.title}</h2>
          <p className="mt-2 text-sm leading-relaxed text-slate-400">{request.description}</p>
          <div className="mt-4 grid gap-2 sm:grid-cols-2">
            <div className="rounded-xl border border-cyan-500/10 bg-slate-900/70 p-3">
              <span className="block font-mono text-[10px] uppercase tracking-widest text-slate-500">risco</span>
              <strong className="mt-1 block text-sm text-slate-100">{request.risk}</strong>
            </div>
            <div className="rounded-xl border border-cyan-500/10 bg-slate-900/70 p-3">
              <span className="block font-mono text-[10px] uppercase tracking-widest text-slate-500">snapshot</span>
              <strong className="mt-1 block text-sm text-slate-100">{request.snapshot ? "obrigatorio" : "nao altera snapshot"}</strong>
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 border-t border-white/5 p-4">
          <button
            onClick={onCancel}
            className="rounded-xl border border-slate-600/60 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-400/70"
          >
            Cancelar
          </button>
          <button
            onClick={onConfirm}
            className="rounded-xl border border-cyan-400/50 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15"
          >
            Confirmar
          </button>
        </div>
      </section>
    </div>
  );
}

function ViewFallback() {
  return <div className="glass-panel h-44 animate-pulse" />;
}
