import { Suspense, lazy, useCallback, useState } from "react";
import { ShieldCheck } from "lucide-react";
import { canUseAutomaticGameMode, type useAuth } from "@/hooks/useAuth";
import type { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { useI18n } from "@/i18n";
import { fromLegacyView, route, type LegacyView, type Route } from "@/lib/routes";
import type { DiskNearFullInfo } from "@/services/insights";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { History } from "@/components/screens/history/History";
import { MyPC } from "@/components/screens/mypc/MyPC";
import type { RequestConfirmation, RunConfirmed } from "./useConfirmation";

const Dashboard = lazy(() => import("@/components/analystblaze/views/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("@/components/analystblaze/views/Telemetry").then((module) => ({ default: module.Telemetry })));
const Insights = lazy(() => import("@/components/analystblaze/views/Insights").then((module) => ({ default: module.Insights })));
const LocalControls = lazy(() => import("@/components/analystblaze/views/LocalControls").then((module) => ({ default: module.LocalControls })));
const DiskExplorer = lazy(() => import("@/components/analystblaze/views/DiskExplorer").then((module) => ({ default: module.DiskExplorer })));
const Network = lazy(() => import("@/components/analystblaze/views/Network").then((module) => ({ default: module.Network })));
const Settings = lazy(() => import("@/components/analystblaze/views/Settings").then((module) => ({ default: module.Settings })));

function ViewFallback() {
  return <div className="h-44 animate-pulse rounded-card border border-line bg-surface" />;
}

/**
 * O "outlet" do AppShell: escolhe a tela pela rota e liga cada view atual aos
 * comandos do agente, com a mesma confirmação de antes. Etapa 1 do doc 15:
 * as views não mudaram; só passaram a morar na casca nova.
 */
export function ScreenOutlet({
  current,
  navigate,
  auth,
  telemetry,
  runConfirmed,
  requestConfirmation,
  shadowConsentNeeded,
  resolveShadowConsent,
}: {
  current: Route;
  navigate: (next: Route) => void;
  auth: ReturnType<typeof useAuth>;
  telemetry: ReturnType<typeof useAgentTelemetry>;
  runConfirmed: RunConfirmed;
  requestConfirmation: RequestConfirmation;
  shadowConsentNeeded: boolean;
  resolveShadowConsent: (choice: "auto" | "manual") => Promise<void>;
}) {
  const { t } = useI18n();
  const [focusDiskUsage, setFocusDiskUsage] = useState(false);
  const [focusTracerouteTarget, setFocusTracerouteTarget] = useState<string | null>(null);
  const [diskNearFullInfo, setDiskNearFullInfo] = useState<DiskNearFullInfo | null>(null);

  const handleViewChange = useCallback((view: LegacyView) => navigate(fromLegacyView(view)), [navigate]);

  const openDiskUsageDetails = useCallback(() => {
    setFocusDiskUsage(true);
    navigate(route("mypc", "disk"));
  }, [navigate]);

  const openNetworkDetails = useCallback(
    (autoTracerouteTarget?: string) => {
      if (autoTracerouteTarget) setFocusTracerouteTarget(autoTracerouteTarget);
      navigate(route("mypc", "network"));
    },
    [navigate],
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
    (actionName: string, context?: Record<string, unknown>) => {
      if (actionName === "APPLY_GAME_MODE") {
        return runConfirmed(
          {
            title: "Ativar Modo Gamer",
            description: "O agente aplica jogo em alta prioridade, reduz fundo, ajusta energia/visual, faz limpeza segura reversível e mede rede. A restauração fica pronta para quando você sair do jogo.",
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
            description: "Move arquivos temporários destravados com pelo menos 5 minutos para quarentena e tenta incluir a TEMP do Windows quando o helper permitir.",
            risk: "sensivel",
            snapshot: true,
          },
          auth.cleanTempDeep,
        );
      }
      if (actionName === "ENABLE_SCHEDULED_DEFRAG") {
        return runConfirmed(
          {
            title: "Reativar otimização automática de disco",
            description: "Reativa a tarefa nativa do Windows que desfragmenta seu HD periodicamente, mantendo a leitura de arquivos rápida com o tempo. Exige o helper privilegiado instalado.",
            risk: "sensivel",
            snapshot: false,
          },
          auth.enableScheduledDefrag,
        );
      }
      if (actionName === "START_SYSTEM_FILE_CHECK") {
        return runConfirmed(
          {
            title: "Verificar arquivos de sistema (SFC)",
            description: "Executa sfc /scannow. Pode levar vários minutos e usa privilégios administrativos - não interrompa nem reinicie o PC enquanto estiver em andamento.",
            risk: "sensivel",
            snapshot: false,
          },
          auth.startSystemFileCheck,
        );
      }
      if (actionName === "RESTART_PNP_DEVICE") {
        const deviceId = typeof context?.deviceId === "string" ? context.deviceId : null;
        if (!deviceId) {
          return Promise.reject(new Error("deviceId ausente para RESTART_PNP_DEVICE."));
        }
        return runConfirmed(
          {
            title: "Reiniciar dispositivo",
            description: "Desativa e reativa o dispositivo (o mesmo que fazer manualmente no Gerenciador de Dispositivos). Resolve boa parte dos erros de driver, mas nem sempre - alguns só somem com uma reinicialização completa do Windows.",
            risk: "sensivel",
            snapshot: false,
          },
          () => auth.restartPnpDevice(deviceId),
        );
      }
      if (actionName === "DISABLE_GAME_DVR") {
        return runConfirmed(
          {
            title: "Desativar gravação do Game Bar",
            description: "Desliga a gravação em segundo plano do Xbox Game Bar (Game DVR). Reversível - a chave anterior fica salva no histórico de ações.",
            risk: "sensivel",
            snapshot: true,
          },
          auth.disableGameDvr,
        );
      }
      if (actionName === "REPAIR_SERVICE") {
        const serviceName = typeof context?.serviceName === "string" ? context.serviceName : null;
        if (!serviceName) {
          return Promise.reject(new Error("serviceName ausente para REPAIR_SERVICE."));
        }
        return runConfirmed(
          {
            title: "Tentar corrigir serviço",
            description: "Para e reinicia o serviço, depois confere se ele realmente continua ativo antes de dizer que funcionou. Pode não resolver causas mais profundas (instalação corrompida, dependência ausente).",
            risk: "sensivel",
            snapshot: true,
          },
          () => auth.repairService(serviceName),
        );
      }
      if (actionName === "DISABLE_SERVICE_PERMANENTLY") {
        const serviceName = typeof context?.serviceName === "string" ? context.serviceName : null;
        if (!serviceName) {
          return Promise.reject(new Error("serviceName ausente para DISABLE_SERVICE_PERMANENTLY."));
        }
        return runConfirmed(
          {
            title: "Desativar serviço permanentemente",
            description: "O serviço para de iniciar sozinho (Tipo de inicialização = Desativado). Reversível pelo histórico de ações, mas só use se tiver certeza de que não precisa dele.",
            risk: "sensivel",
            snapshot: true,
          },
          () => auth.disableServicePermanently(serviceName),
        );
      }
      if (actionName === "THROTTLE_DEFENDER_SCANS") {
        return runConfirmed(
          {
            title: "Limitar verificações do Defender",
            description: "Reduz a CPU que as verificações agendadas do Windows Defender podem usar (de 50% para 20%) e faz elas rodarem só com o PC ocioso. A proteção em tempo real não muda. Reversível pelo próprio card.",
            risk: "sensivel",
            snapshot: true,
          },
          auth.throttleDefenderScans,
        );
      }
      if (actionName === "RESTORE_DEFENDER_SCAN_SETTINGS") {
        return runConfirmed(
          {
            title: "Restaurar verificações do Defender",
            description: "Devolve as preferências de verificação do Defender exatamente como estavam antes do AnalystBlaze limita-las.",
            risk: "sensivel",
            snapshot: false,
          },
          auth.restoreDefenderScanSettings,
        );
      }
      if (actionName === "RENEW_DEFENDER_DEFINITIONS") {
        return runConfirmed(
          {
            title: "Renovar definições do Defender",
            description: "Remove o conjunto atual de definições de vírus (que pode estar corrompido) e baixa um novo. O Defender fica alguns minutos sem definições até o download terminar - não abra arquivos suspeitos nesse intervalo.",
            risk: "sensivel",
            snapshot: false,
          },
          auth.renewDefenderDefinitions,
        );
      }
      if (actionName === "SCHEDULE_VOLUME_CHECK") {
        const driveLetter = typeof context?.driveLetter === "string" ? context.driveLetter : null;
        if (!driveLetter) {
          return Promise.reject(new Error("driveLetter ausente para SCHEDULE_VOLUME_CHECK."));
        }
        return runConfirmed(
          {
            title: `Verificar o disco ${driveLetter}: na próxima reinicialização`,
            description: "Marca a unidade para o Windows rodar a verificação e reparo do sistema de arquivos (chkdsk) antes de abrir, na próxima vez que você reiniciar. Pode levar alguns minutos a mais nesse boot.",
            risk: "sensivel",
            snapshot: false,
          },
          () => auth.scheduleVolumeCheck(driveLetter),
        );
      }
      return Promise.reject(new Error(`Ação não suportada localmente: ${actionName}`));
    },
    [runConfirmed, activateGameModeWithUpsell, auth],
  );

  if (!auth.ready) {
    return <div className="text-sm text-fg-3">{t("app.loading")}</div>;
  }

  switch (current.screen) {
    case "dashboard":
      return (
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
                  description: "O agente aplica jogo em alta prioridade, reduz fundo, ajusta energia/visual, faz limpeza segura reversível e mede rede. A restauração fica pronta para quando você sair do jogo.",
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
                  description: "Restaura snapshots da sessão ativa de Modo Gamer, incluindo plano de energia e prioridades de processos quando disponíveis.",
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
      );
    case "history":
      return <History onOpenControls={() => handleViewChange("controls")} />;
    case "insights":
      return (
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
      );
    case "controls":
      return (
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
                  description: "O agente vai tentar desfazer alterações pendentes de energia, limpeza, apps de inicialização e serviços.",
                  risk: "seguro",
                  snapshot: false,
                },
                auth.restoreOptimizations,
              )
            }
            onStartSystemFileCheck={() =>
              runConfirmed(
                {
                  title: "Verificar arquivos de sistema (SFC)",
                  description: "Executa sfc /scannow. Pode levar vários minutos e usa privilégios administrativos - não interrompa nem reinicie o PC enquanto estiver em andamento.",
                  risk: "sensivel",
                  snapshot: false,
                },
                auth.startSystemFileCheck,
              )
            }
            onStartDismRestoreHealth={() =>
              runConfirmed(
                {
                  title: "Reparar componentes do Windows (DISM)",
                  description: "Executa DISM /Online /Cleanup-Image /RestoreHealth. Pode levar bem mais tempo que o SFC e precisa de conexão com a internet - não interrompa nem reinicie o PC enquanto estiver em andamento.",
                  risk: "sensivel",
                  snapshot: false,
                },
                auth.startDismRestoreHealth,
              )
            }
            onDisableStartup={(name, location) =>
              runConfirmed(
                {
                  title: `Desativar ${name}`,
                  description: `Remove o app da inicialização do Windows em ${location ?? "registro detectado"} e cria snapshot para restaurar depois.`,
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.disableStartup(name, location),
              )
            }
            onRestoreStartup={(name) =>
              runConfirmed(
                {
                  title: name ? `Restaurar ${name}` : "Restaurar apps de inicialização",
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
                  title: `Parar serviço ${name}`,
                  description: "O serviço será parado apenas se passar pela denylist local. Um snapshot guarda se ele estava rodando antes.",
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.stopService(name),
              )
            }
            onRestoreService={(name) =>
              runConfirmed(
                {
                  title: name ? `Restaurar serviço ${name}` : "Restaurar serviços",
                  description: "Tenta religar apenas serviços que estavam rodando antes da ação.",
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
                  description: "Reduz animacoes, transparencia e efeitos visuais do Windows para deixar o sistema mais leve (reversível por snapshot). Pode causar um pequeno engasgo de um instante em outros programas abertos, principalmente navegadores - e o Windows avisando todo mundo da mudança, um comportamento normal do sistema, não um erro.",
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
                  description: "Move arquivos temporários destravados com pelo menos 5 minutos para quarentena e tenta incluir a TEMP do Windows quando o helper permitir.",
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
                  description: "Apaga permanentemente a quarentena de limpeza. Depois disso esses arquivos não poderão ser restaurados.",
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
                  description: "Restaura snapshots da sessão ativa de Modo Gamer, incluindo plano de energia e prioridades de processos quando disponíveis.",
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
                  description: "Restaura snapshots criados pelo perfil PC limpo/rápido, incluindo visual, prioridades e inicialização atrasada quando existirem.",
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
                    ? "Apaga permanentemente a quarentena para liberar espaço real."
                    : "Move arquivos elegiveis desta categoria para quarentena reversível.",
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
                  description: `Remove temporariamente ${name} da inicialização direta e coloca na fila local do AnalystBlaze para iniciar depois.`,
                  risk: "sensivel",
                  snapshot: true,
                },
                () => auth.delayStartup(name, location),
              )
            }
            onRestoreDelayedStartupApp={(name) =>
              runConfirmed(
                {
                  title: name ? `Restaurar ${name}` : "Restaurar inicialização atrasada",
                  description: "Restaura o valor de inicialização salvo em snapshot local.",
                  risk: "seguro",
                  snapshot: false,
                },
                () => auth.restoreDelayedStartup(name),
              )
            }
          />
        </Suspense>
      );
    case "settings":
      return (
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
      );
    case "mypc": {
      const tab = current.tab ?? "performance";
      return (
        <MyPC tab={tab} onTabChange={(next) => navigate(route("mypc", next))}>
          <Suspense fallback={<ViewFallback />}>
            {tab === "performance" ? (
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
            ) : tab === "disk" ? (
              <DiskExplorer
                autoScan={focusDiskUsage}
                onAutoScanHandled={() => setFocusDiskUsage(false)}
                onDiskNearFullDetected={setDiskNearFullInfo}
              />
            ) : tab === "network" ? (
              <Network
                busy={auth.busy}
                isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                autoTracerouteTarget={focusTracerouteTarget}
                onAutoTracerouteHandled={() => setFocusTracerouteTarget(null)}
                onFlushDnsCache={() =>
                  runConfirmed(
                    {
                      title: "Limpar cache de DNS",
                      description: "Executa ipconfig /flushdns. Não exige admin e não altera nenhum estado que precise de restauração.",
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
                      description: "Troca os servidores DNS do adaptador selecionado. O agente salva a configuração atual em snapshot local para restaurar depois.",
                      risk: "sensivel",
                      snapshot: true,
                    },
                    () => auth.setDnsServers(adapterName, dnsServers),
                  )
                }
                onResetWinsockCatalog={() =>
                  runConfirmed(
                    {
                      title: "Resetar catálogo Winsock",
                      description: "Executa netsh winsock reset. E disruptivo, exige reinicialização do computador e não pode ser desfeito automaticamente pelo agente.",
                      risk: "sensivel",
                      snapshot: false,
                    },
                    auth.resetWinsock,
                  )
                }
                onSetAdapterEnabled={async (adapterName, enabled, context) => {
                  if (context?.riskWarning) {
                    const acknowledged = await requestConfirmation({
                      title: "Atenção: essa interface parece estar em uso",
                      description: context.riskWarning,
                      risk: "sensivel",
                      snapshot: false,
                    });
                    if (!acknowledged) return false;
                  }
                  const baseDescription = enabled
                    ? "Reativa o adaptador de rede selecionado."
                    : "Desativa o adaptador de rede selecionado para forçar o tráfego a sair por outro adaptador. O agente salva o estado atual em snapshot local para restaurar depois.";
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
                      title: "Aplicar otimização de TCP",
                      description: "Ajusta parâmetros da pilha TCP do Windows (auto-tuning, ECN, congestionamento ou registro por adaptador). O agente salva a configuração atual em snapshot local; se ninguém confirmar, e revertido automaticamente.",
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
                      description: "Fecha todos os programas abertos e reinicia o Windows em alguns segundos. Necessário para aplicar ajustes de registro na rede.",
                      risk: "sensivel",
                      snapshot: false,
                    },
                    auth.restartWindowsNow,
                  )
                }
              />
            ) : (
              <EmptyState
                icon={<ShieldCheck />}
                title={t("mypc.healthPendingTitle")}
                reason={t("mypc.healthPendingReason")}
                action={
                  <Button variant="outline" onClick={() => handleViewChange("insights")}>
                    {t("mypc.healthOpenInsights")}
                  </Button>
                }
              />
            )}
          </Suspense>
        </MyPC>
      );
    }
  }
}
