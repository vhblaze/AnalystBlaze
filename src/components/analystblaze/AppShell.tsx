import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "./Sidebar";
import { TopBar, type TopBarNotification, type TopBarSearchItem } from "./TopBar";
import { UpdateNotice } from "./UpdateNotice";
import { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { canUseAutomaticGameMode, useAuth } from "@/hooks/useAuth";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { isUpdateDismissedNow, useUpdater } from "@/hooks/useUpdater";
import { useI18n } from "@/i18n";
import {
  getActiveAnnouncements,
  getPrivilegedHelperStatus,
  installPrivilegedHelper,
  isTauriRuntime,
  listenToAnnouncements,
  listenToRemoteCommandConfirmation,
  resolveRemoteCommandConfirmation,
  type Announcement,
  type RemoteCommandConfirmationRequest,
} from "@/services/tauri/agent";

export type ViewKey = "dashboard" | "telemetry" | "insights" | "controls" | "settings";

const Dashboard = lazy(() => import("./views/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("./views/Telemetry").then((module) => ({ default: module.Telemetry })));
const Insights = lazy(() => import("./views/Insights").then((module) => ({ default: module.Insights })));
const LocalControls = lazy(() => import("./views/LocalControls").then((module) => ({ default: module.LocalControls })));
const Settings = lazy(() => import("./views/Settings").then((module) => ({ default: module.Settings })));

export function AppShell() {
  const [view, setView] = useState<ViewKey>("dashboard");
  const [focusDiskUsage, setFocusDiskUsage] = useState(false);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const [remoteConfirmationQueue, setRemoteConfirmationQueue] = useState<RemoteCommandConfirmationRequest[]>([]);
  const [announcements, setAnnouncements] = useState<Announcement[]>([]);
  const [dismissedAnnouncementIds, setDismissedAnnouncementIds] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("analystblaze.dismissedAnnouncements") ?? "[]");
    } catch {
      return [];
    }
  });
  const helperBootstrapStartedRef = useRef(false);
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

  const titles = useMemo<Record<ViewKey, string>>(
    () => ({
      dashboard: t("nav.dashboard"),
      telemetry: t("nav.telemetry"),
      insights: t("nav.insights"),
      controls: t("nav.controls"),
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
    handleViewChange("controls");
  }, [handleViewChange]);

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
    [remoteConfirmationQueue, updater.status, announcements, dismissedAnnouncementIds, t],
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

  useEffect(() => {
    if (!auth.ready || helperBootstrapStartedRef.current || !isTauriRuntime()) return;
    helperBootstrapStartedRef.current = true;

    let disposed = false;
    const bootstrapKey = "analystblaze.helper.bootstrap.v1";

    const bootstrapHelper = async () => {
      try {
        const currentStatus = await getPrivilegedHelperStatus();
        if (disposed || (currentStatus.available && !currentStatus.requiresUpdate)) return;
        const previousDecision = window.localStorage.getItem(bootstrapKey);
        if (previousDecision === "installed" || previousDecision === "dismissed") return;
        if (!currentStatus.canRequestUac) return;

        const approved = await requestConfirmation({
          title: "Configurar helper admin",
          description: "O AnalystBlaze pode instalar o helper local agora. Isso so fica disponivel em instalacao per-machine/Program Files; o Windows vai pedir UAC uma vez.",
          risk: "sensivel",
          snapshot: false,
        });
        if (disposed) return;
        if (!approved) {
          window.localStorage.setItem(bootstrapKey, "dismissed");
          return;
        }

        toast({
          title: "Configurando helper admin",
          description: "Confirme o UAC do Windows para concluir a configuracao.",
        });
        const nextStatus = await installPrivilegedHelper();
        window.localStorage.setItem(bootstrapKey, nextStatus.available ? "installed" : "attempted");
        toast({
          title: nextStatus.available ? "Helper admin pronto" : "Helper admin precisa de atencao",
          description: nextStatus.message,
        });
      } catch (error) {
        window.localStorage.setItem(bootstrapKey, "attempted");
        toast({
          title: "Helper admin nao foi configurado",
          description: String(error),
          variant: "destructive",
        });
      }
    };

    void bootstrapHelper();

    return () => {
      disposed = true;
    };
  }, [auth.ready, requestConfirmation]);

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
            <UpdateNotice
              status={updater.status}
              busy={auth.busy}
              onUpdateNow={handleUpdateNow}
              onLater={handleUpdateLater}
            />
            {!auth.ready ? (
              <div className="text-sm text-slate-500">{t("app.loading")}</div>
            ) : view === "dashboard" ? (
              <Suspense fallback={<ViewFallback />}>
                <Dashboard
                  user={auth.user}
                  status={auth.status}
                  telemetry={telemetry}
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
                />
              </Suspense>
            ) : view === "insights" ? (
              <Suspense fallback={<ViewFallback />}>
                <Insights
                  telemetry={telemetry}
                  onOpenDiskUsage={openDiskUsageDetails}
                  onApplyInsightActionLocally={applyInsightActionLocally}
                  onRequestAgentApplyInsight={auth.requestAgentApplyInsight}
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
                  onActivateFocusMode={(profile) =>
                    runConfirmed(
                      {
                        title: "Ativar Modo Foco",
                        description: "Cria uma sessao local com quiet mode, uploads nao criticos atrasados, scans pesados pausados e restauracao automatica.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.activateFocus(profile),
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
                        description: "Reduz animacoes, transparencia, Aero Peek e efeitos leves do Explorer usando valores HKCU reversiveis por snapshot.",
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
                  onRestoreFocusMode={() =>
                    runConfirmed(
                      {
                        title: "Restaurar Modo Foco",
                        description: "Encerra a sessao de foco ativa e restaura snapshots locais de prioridades e eficiencia de processos quando existirem.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreFocus,
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
                  focusDiskUsage={focusDiskUsage}
                  onDiskUsageFocused={() => setFocusDiskUsage(false)}
                />
              </Suspense>
            ) : (
              <Suspense fallback={<ViewFallback />}>
                <Settings
                  user={auth.user}
                  status={auth.status}
                  message={auth.message}
                  busy={auth.busy}
                  syncingPlan={auth.syncingPlan}
                  onLogin={auth.login}
                  onLogout={auth.logout}
                  onOpenAccountSettings={auth.openAccountSettings}
                  onOpenBilling={auth.openBilling}
                  onStartAgent={auth.start}
                  onCollectSample={auth.collectSample}
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
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={request.title}
        className="w-full max-w-lg rounded-2xl border border-cyan-400/20 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)]"
      >
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
        <div className="mt-6 flex justify-end gap-2">
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
