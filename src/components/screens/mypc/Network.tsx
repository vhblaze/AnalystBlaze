import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { AdvancedDisclosure } from "@/components/ui/advanced-disclosure";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusPill } from "@/components/ui/status-pill";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatMs, formatNumber, formatPercent } from "@/lib/format";
import { cn } from "@/lib/utils";
import {
  benchmarkDnsServers,
  checkAdapterDisableGuard,
  confirmNetworkTune,
  getPendingNetworkTune,
  isTauriRuntime,
  listenToDnsOptimizationSuggested,
  revertNetworkTune,
  runNetworkTraceroute,
  type DnsBenchmarkResult,
  type DnsOptimizationSuggestion,
  type NetworkAdapterSummary,
  type NetworkDiagnostics,
  type NetworkTuneRequest,
  type NetworkTuneSession,
  type TracerouteResult,
} from "@/services/tauri/agent";
import { networkDiagnostics as networkDiagnosticsResource, useNetwork } from "@/features/network/useNetwork";
import { allNetworkAdapters, networkAdapters } from "@/features/network/useAdapters";
import { privilegedHelperStatus, useHelper } from "@/features/helper/useHelper";
import { auditLog, optimizationSnapshots } from "@/features/history/useHistory";
import {
  classifyRouteDiagnosis,
  errorMessage,
  isGameServerHigh,
  NETWORK_LAG_FLAGS,
  summarizeHealth,
  type HealthTone,
  type NetworkLagFlag,
  type RouteVerdict,
} from "./network/diagnosis";
import { LiveMode } from "./network/LiveMode";
import { NetworkAdvanced } from "./network/NetworkAdvanced";
import { RouteDetails, verdictTone } from "./network/RouteDetails";
import { TuneDialog } from "./network/TuneDialog";

type NetworkDiagnosticsCache = {
  adapters: NetworkAdapterSummary[];
  allAdapters: NetworkAdapterSummary[];
  dnsBenchmark: DnsBenchmarkResult[] | null;
};

/** No módulo (não no componente): sobrevive à troca de aba, então a aba volta
 * na hora com a última leitura boa enquanto a releitura roda por trás. Guarda
 * só o último retrato (poucos campos), nunca um histórico. */
let networkDiagnosticsCache: NetworkDiagnosticsCache | null = null;
/** O teste de DNS automático roda no máximo uma vez por sessão do app. */

type ActionOutcome = { success?: boolean; message?: string } | null | undefined;

/**
 * Meu PC › Rede (boards Tela-MeuPC-Rede e Tela-MeuPC-Rede-Avancado). Mesmas
 * funções da view antiga (views/Network.tsx), com outra pele: estado em uma
 * frase, três cards (No jogo, Caminho até a internet, DNS), Modo ao vivo e o
 * Avançado aberto dentro da aba.
 */
export function Network({
  busy,
  isReady,
  onFlushDnsCache,
  onSetDnsServers,
  onResetWinsockCatalog,
  onSetAdapterEnabled,
  onApplyNetworkTune,
  onRestartWindows,
  autoTracerouteTarget,
  onAutoTracerouteHandled,
}: {
  busy: boolean;
  isReady: boolean;
  onFlushDnsCache: () => Promise<unknown>;
  onSetDnsServers: (adapterName: string, dnsServers: string[]) => Promise<unknown>;
  onResetWinsockCatalog: () => Promise<unknown>;
  onSetAdapterEnabled: (
    adapterName: string,
    enabled: boolean,
    context?: { reason?: string; riskWarning?: string },
  ) => Promise<unknown>;
  onApplyNetworkTune: (request: NetworkTuneRequest) => Promise<unknown>;
  onRestartWindows: () => Promise<unknown>;
  /** Vem do "Diagnosticar causa" de Insights: abre o diagnóstico de rota e roda até este IP. */
  autoTracerouteTarget?: string | null;
  onAutoTracerouteHandled?: () => void;
}) {
  const { t } = useI18n();
  const track = useTelemetry("network");
  const runtimeAvailable = isTauriRuntime();

  // A mesma leitura de Insights (features/network) e do componente de sistema (features/helper).
  const diagnostics = useNetwork({ auto: false }).diagnostics;
  const helper = useHelper({ auto: false });

  const [adapters, setAdapters] = useState<NetworkAdapterSummary[]>(() => networkDiagnosticsCache?.adapters ?? []);
  const [allAdapters, setAllAdapters] = useState<NetworkAdapterSummary[]>(() => networkDiagnosticsCache?.allAdapters ?? []);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);
  const [firstReadDone, setFirstReadDone] = useState(false);
  const [actionKey, setActionKey] = useState<string | null>(null);
  const [adapterBusyName, setAdapterBusyName] = useState<string | null>(null);
  const [dnsAdapterName, setDnsAdapterName] = useState("");

  const [routeOpen, setRouteOpen] = useState(false);
  const [tracerouteTarget, setTracerouteTarget] = useState("1.1.1.1");
  const [tracerouteResult, setTracerouteResult] = useState<TracerouteResult | null>(null);
  const [tracerouteBusy, setTracerouteBusy] = useState(false);
  const [tracerouteError, setTracerouteError] = useState<string | null>(null);

  const [dnsResults, setDnsResults] = useState<DnsBenchmarkResult[] | null>(() => networkDiagnosticsCache?.dnsBenchmark ?? null);
  const [dnsBusy, setDnsBusy] = useState(false);
  const [dnsError, setDnsError] = useState<string | null>(null);
  const [dnsSuggestion, setDnsSuggestion] = useState<DnsOptimizationSuggestion | null>(null);

  const [tuneBusy, setTuneBusy] = useState(false);
  const [tuneError, setTuneError] = useState<string | null>(null);
  const [tuneSession, setTuneSession] = useState<NetworkTuneSession | null>(null);
  const [tuneSecondsLeft, setTuneSecondsLeft] = useState<number | null>(null);
  const [tuneRevertedNeedsReboot, setTuneRevertedNeedsReboot] = useState(false);

  const agentReason = runtimeAvailable ? null : t("mypc.network.needsAgent");
  const helperReason = helper.ready ? null : t("mypc.network.needsHelper");
  const locked = busy || diagnosticsBusy || actionKey !== null;

  const health = useMemo(() => summarizeHealth(diagnostics?.recommendations ?? []), [diagnostics?.recommendations]);
  const routeVerdict = useMemo(() => (tracerouteResult ? classifyRouteDiagnosis(tracerouteResult) : null), [tracerouteResult]);

  const refreshNetwork = useCallback(async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsError(null);
    try {
      const [nextNetwork, nextAdapters, nextAllAdapters] = await Promise.all([
        networkDiagnosticsResource.refresh(),
        networkAdapters.refresh(),
        allNetworkAdapters.refresh(),
        privilegedHelperStatus.refresh(),
      ]);
      setAdapters(nextAdapters);
      setAllAdapters(nextAllAdapters);
      networkDiagnosticsCache = {
        adapters: nextAdapters,
        allAdapters: nextAllAdapters,
        dnsBenchmark: networkDiagnosticsCache?.dnsBenchmark ?? null,
      };
      setDnsAdapterName((current) => {
        if (current && nextAdapters.some((adapter) => adapter.name === current)) return current;
        return nextNetwork.adapter_name ?? nextAdapters[0]?.name ?? "";
      });
      setFirstReadDone(true);
      track("network_refreshed");
    } catch (error) {
      setDiagnosticsError(errorMessage(error));
      // Uma falha passageira não apaga uma leitura boa já em tela.
      if (!networkDiagnosticsCache) {
        networkDiagnosticsResource.set(undefined);
        setAdapters([]);
        setAllAdapters([]);
      }
    } finally {
      setDiagnosticsBusy(false);
    }
  }, [track]);

  useEffect(() => {
    void refreshNetwork();
  }, [refreshNetwork]);

  // Sessão de TCP pendente (ex.: aplicada antes de reiniciar).
  useEffect(() => {
    if (!runtimeAvailable) return;
    getPendingNetworkTune()
      .then((session) => setTuneSession(session))
      .catch(() => undefined);
  }, [runtimeAvailable]);

  // Sugestão de DNS do motor local: só sugere, nunca aplica sozinho.
  useEffect(() => {
    let dispose: (() => void) | undefined;
    let disposed = false;
    listenToDnsOptimizationSuggested((suggestion) => setDnsSuggestion(suggestion)).then((stop) => {
      if (disposed) stop();
      else dispose = stop;
    });
    return () => {
      disposed = true;
      dispose?.();
    };
  }, []);

  const refreshHistory = () => Promise.allSettled([auditLog.refresh(), optimizationSnapshots.refresh()]);

  /** Roda uma ação que já pede confirmação no app; aviso em toast, nunca janela. */
  const runAction = async (key: string, action: () => Promise<unknown>, done: string): Promise<boolean> => {
    setActionKey(key);
    try {
      const result = await action();
      if (result === false) {
        toast({ title: t("mypc.network.cancelled") });
        return false;
      }
      const outcome = result as ActionOutcome;
      if (outcome && typeof outcome === "object" && outcome.success === false) {
        toast({ title: t("mypc.network.failed"), description: outcome.message, variant: "destructive" });
        return false;
      }
      toast({ title: done });
      await refreshNetwork();
      return true;
    } catch (error) {
      toast({ title: t("mypc.network.failed"), description: errorMessage(error), variant: "destructive" });
      return false;
    } finally {
      void refreshHistory();
      setActionKey(null);
    }
  };

  /** O cliente do componente de sistema já tenta de novo antes de desistir; se
   * ainda assim disser "estava reiniciando", a mudança pode ter entrado logo
   * depois. Confere o estado real mais duas vezes antes de dar como falha. */
  const runWithTransientVerify = async (action: () => Promise<unknown>, done: string, verify: () => Promise<boolean>) => {
    try {
      const result = await action();
      if (result === false) {
        toast({ title: t("mypc.network.cancelled") });
        return;
      }
      toast({ title: done });
      await refreshNetwork();
    } catch (error) {
      const message = errorMessage(error);
      if (message.includes("estava reiniciando")) {
        for (const delayMs of [3000, 4000]) {
          await new Promise((resolve) => setTimeout(resolve, delayMs));
          try {
            if (await verify()) {
              toast({ title: done });
              await refreshNetwork();
              return;
            }
          } catch {
            // A conferência falhar não muda o resultado: tenta no próximo intervalo.
          }
        }
      }
      toast({ title: t("mypc.network.failed"), description: message, variant: "destructive" });
    } finally {
      void refreshHistory();
    }
  };

  const adapterIsEnabled = async (adapterName: string): Promise<boolean | null> => {
    const adapter = (await allNetworkAdapters.refresh()).find((item) => item.name === adapterName);
    if (!adapter) return null;
    return (adapter.status ?? "").toLowerCase() !== "disabled";
  };

  /** Antes de desativar: avisa se um jogo está na frente ou se o adaptador tem tráfego agora. */
  const disableRiskWarning = async (adapterName: string): Promise<string | undefined> => {
    try {
      const guard = await checkAdapterDisableGuard(adapterName);
      if (!guard.gameInForeground && !guard.adapterHasActiveTraffic) return undefined;
      const reasons = [
        guard.gameInForeground
          ? t("mypc.network.advanced.adapters.riskGame", {
              game: guard.gameProcessName ?? t("mypc.network.advanced.adapters.riskGameUnknown"),
            })
          : null,
        guard.adapterHasActiveTraffic ? t("mypc.network.advanced.adapters.riskTraffic") : null,
      ].filter((value): value is string => value != null);
      const joined = reasons.join(t("mypc.network.advanced.adapters.riskJoin"));
      return t("mypc.network.advanced.adapters.riskWarning", {
        reasons: joined.charAt(0).toUpperCase() + joined.slice(1),
        adapter: adapterName,
      });
    } catch {
      // Se a checagem falhar, segue para a confirmação normal (a ação continua reversível).
      return undefined;
    }
  };

  const toggleAdapter = async (adapterName: string, enabled: boolean, reason?: string) => {
    setAdapterBusyName(adapterName);
    try {
      const riskWarning = enabled ? undefined : await disableRiskWarning(adapterName);
      const context = reason || riskWarning ? { reason, riskWarning } : undefined;
      await runWithTransientVerify(
        () => onSetAdapterEnabled(adapterName, enabled, context),
        enabled
          ? t("mypc.network.advanced.adapters.enabled", { adapter: adapterName })
          : t("mypc.network.advanced.adapters.disabled", { adapter: adapterName }),
        async () => (await adapterIsEnabled(adapterName)) === enabled,
      );
    } finally {
      setAdapterBusyName(null);
    }
  };

  // "Desativar <VPN>" na faixa: só como causa provável, quando há o adaptador
  // virtual E alguma métrica de instabilidade acima do limite (mesma regra do Insight).
  const activeLagFlags = NETWORK_LAG_FLAGS.filter((flag) => diagnostics?.recommendations?.includes(flag));
  const problemAdapter =
    diagnostics?.adapter_name &&
    diagnostics.recommendations?.includes("vpn_or_virtual_adapter_active") &&
    activeLagFlags.length > 0
      ? diagnostics.adapter_name
      : null;
  const lagMetricText = (flag: NetworkLagFlag) => {
    if (flag === "packet_loss_detected")
      return t("mypc.network.band.metricLoss", { value: `${formatNumber(diagnostics?.packet_loss_percent ?? 0)}%` });
    if (flag === "jitter_high")
      return t("mypc.network.band.metricJitter", { value: formatMs(diagnostics?.jitter_ms, t("mypc.network.lessThanOneMs")) });
    return t("mypc.network.band.metricLatency", { value: formatMs(diagnostics?.external_latency_ms, t("mypc.network.lessThanOneMs")) });
  };
  const lagMetrics = activeLagFlags.map(lagMetricText).join(", ");

  const disableProblemAdapter = async () => {
    if (!problemAdapter) return;
    await toggleAdapter(
      problemAdapter,
      false,
      t("mypc.network.band.problemAdapterReason", { adapter: problemAdapter, metrics: lagMetrics }),
    );
    track("hero_disable_adapter", { adapter: problemAdapter, metrics: lagMetrics });
  };

  const applyDns = (adapterName: string, servers: string[]) =>
    runAction("dns", () => onSetDnsServers(adapterName, servers), t("mypc.network.advanced.dnsManual.done"));

  const applyBenchmarkedDns = async (server: string) => {
    if (!dnsAdapterName) {
      toast({ title: t("mypc.network.dns.selectAdapter") });
      return;
    }
    await runAction(`dns:${server}`, () => onSetDnsServers(dnsAdapterName, [server]), t("mypc.network.dns.changed"));
  };

  const applyDnsSuggestion = async () => {
    if (!dnsSuggestion) return;
    const ok = await runAction(
      "dnsSuggestion",
      () => onSetDnsServers(dnsSuggestion.adapterName, [dnsSuggestion.dnsServer]),
      t("mypc.network.dns.suggestionApplied", { server: dnsSuggestion.dnsLabel }),
    );
    if (ok) setDnsSuggestion(null);
  };

  const runDnsBenchmark = useCallback(async () => {
    setDnsBusy(true);
    setDnsError(null);
    try {
      const results = await benchmarkDnsServers(networkDiagnosticsResource.getState().data?.dns_servers ?? []);
      setDnsResults(results);
      if (networkDiagnosticsCache) networkDiagnosticsCache.dnsBenchmark = results;
      track("dns_benchmark_run", { count: results.length });
    } catch (error) {
      setDnsError(errorMessage(error));
    } finally {
      setDnsBusy(false);
    }
  }, [track]);

  // O teste de DNS só roda quando a pessoa pede: o teste automático é a
  // automação "Sugerir DNS mais rápido", que ela liga ou desliga em Controles.

  const runTraceroute = async (overrideTarget?: string) => {
    setTracerouteBusy(true);
    setTracerouteError(null);
    try {
      const result = await runNetworkTraceroute((overrideTarget ?? tracerouteTarget).trim() || undefined);
      setTracerouteResult(result);
      track("traceroute_run", { target: result.target, hops: result.hops.length });
    } catch (error) {
      setTracerouteError(errorMessage(error));
    } finally {
      setTracerouteBusy(false);
    }
  };

  const openRoute = (target?: string) => {
    setRouteOpen(true);
    if (target) {
      setTracerouteTarget(target);
      void runTraceroute(target);
    }
  };

  useEffect(() => {
    if (!routeOpen) return;
    document.getElementById("network-route-details")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [routeOpen]);

  // Insights entrega o IP do servidor do jogo: abre a rota e já roda.
  useEffect(() => {
    if (!autoTracerouteTarget) return;
    openRoute(autoTracerouteTarget);
    onAutoTracerouteHandled?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoTracerouteTarget]);

  // ----- Otimização de TCP: sessão de 30 s com reversão automática -----
  const revertTune = async () => {
    const wasRebootTune = tuneSession?.requiresReboot ?? false;
    setTuneBusy(true);
    try {
      await revertNetworkTune();
    } catch (error) {
      setTuneError(errorMessage(error));
    } finally {
      setTuneSession(null);
      setTuneRevertedNeedsReboot(wasRebootTune);
      setTuneBusy(false);
      void refreshHistory();
    }
  };

  useEffect(() => {
    if (!tuneSession || tuneSession.status !== "pending" || !tuneSession.countdownStarted) {
      setTuneSecondsLeft(null);
      return;
    }
    const tick = () => {
      const secondsLeft = Math.max(0, Math.ceil((tuneSession.confirmDeadline * 1000 - Date.now()) / 1000));
      setTuneSecondsLeft(secondsLeft);
      if (secondsLeft <= 0) void revertTune();
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tuneSession]);

  const applyTune = async (request: NetworkTuneRequest) => {
    setTuneBusy(true);
    setTuneError(null);
    setTuneRevertedNeedsReboot(false);
    try {
      const result = await onApplyNetworkTune(request);
      if (result === false) return; // a pessoa cancelou a confirmação
      const outcome = result as { success?: boolean; message?: string; details?: { session?: NetworkTuneSession } } | undefined;
      if (outcome && outcome.success === false) {
        setTuneError(outcome.message ?? t("mypc.network.advanced.tcp.applyFailed"));
        return;
      }
      setTuneSession(outcome?.details?.session ?? null);
      if (outcome?.message) toast({ title: outcome.message });
    } catch (error) {
      setTuneError(errorMessage(error));
    } finally {
      setTuneBusy(false);
      void refreshHistory();
    }
  };

  const confirmTune = async () => {
    setTuneBusy(true);
    try {
      await confirmNetworkTune();
      setTuneSession(null);
      toast({ title: t("mypc.network.advanced.tcp.kept") });
    } catch (error) {
      setTuneError(errorMessage(error));
    } finally {
      setTuneBusy(false);
      void refreshHistory();
    }
  };

  const restartNow = async () => {
    setTuneBusy(true);
    try {
      await onRestartWindows();
    } catch (error) {
      setTuneError(errorMessage(error));
      setTuneBusy(false);
    }
  };

  // Só na primeira leitura, antes de existir qualquer resultado: uma releitura
  // depois disso nunca apaga o que já está na tela.
  const isInitialLoad = diagnosticsBusy && !diagnostics;
  const tunePending = tuneSession?.status === "pending" ? tuneSession : null;
  const awaitingReboot = tunePending ? tunePending.requiresReboot && !tunePending.countdownStarted : false;

  return (
    <div className="flex flex-col gap-4">
      <StatusBand
        diagnostics={diagnostics}
        tone={health.tone}
        headlineCode={health.headlineCode}
        detailCodes={health.detailCodes}
        loading={isInitialLoad}
        refreshing={diagnosticsBusy}
        error={diagnosticsError}
        onRetest={() => void refreshNetwork()}
      >
        {problemAdapter && (
          <div className="flex flex-wrap items-center gap-3 border-t border-line-soft pt-3">
            <p className="min-w-0 flex-1 text-[13px] text-warn">
              {t("mypc.network.band.problemAdapter", { adapter: problemAdapter, metrics: lagMetrics })}
            </p>
            <Button
              size="sm"
              variant="outline"
              disabled={locked || adapterBusyName !== null}
              disabledReason={agentReason ?? helperReason}
              onClick={() => void disableProblemAdapter()}
            >
              {adapterBusyName === problemAdapter ? t("mypc.network.working") : t("mypc.network.band.disableAdapter", { adapter: problemAdapter })}
            </Button>
          </div>
        )}
      </StatusBand>

      {awaitingReboot && (
        <RestartNotice message={t("mypc.network.advanced.tcp.pendingReboot")} busy={tuneBusy} onRestart={() => void restartNow()} />
      )}
      {tuneRevertedNeedsReboot && (
        <RestartNotice
          message={t("mypc.network.advanced.tcp.revertedRestart")}
          busy={tuneBusy}
          onRestart={() => void restartNow()}
          onDismiss={() => setTuneRevertedNeedsReboot(false)}
        />
      )}

      {!isInitialLoad && (
        <div className="grid grid-cols-1 items-stretch gap-4 lg:grid-cols-3">
          <GameCard
            diagnostics={diagnostics}
            busy={tracerouteBusy}
            agentReason={agentReason}
            onDiagnose={(ip) => openRoute(ip)}
          />
          <PathCard diagnostics={diagnostics} trace={tracerouteResult} verdict={routeVerdict} onOpenRoute={() => openRoute()} />
          <DnsCard
            results={dnsResults}
            busy={dnsBusy}
            error={dnsError}
            suggestion={dnsSuggestion}
            locked={locked}
            actionKey={actionKey}
            agentReason={agentReason}
            helperReason={helperReason}
            onRun={() => void runDnsBenchmark()}
            onUse={(server) => void applyBenchmarkedDns(server)}
            onApplySuggestion={() => void applyDnsSuggestion()}
            onDismissSuggestion={() => setDnsSuggestion(null)}
          />
        </div>
      )}

      {routeOpen && (
        <RouteDetails
          target={tracerouteTarget}
          onTargetChange={setTracerouteTarget}
          result={tracerouteResult}
          verdict={routeVerdict}
          busy={tracerouteBusy}
          error={tracerouteError}
          disabledReason={agentReason}
          onRun={() => void runTraceroute()}
          onClose={() => setRouteOpen(false)}
        />
      )}

      <LiveMode isReady={isReady} />

      {!isInitialLoad && (
        <AdvancedDisclosure summary={t("mypc.network.advanced.summary")}>
          <NetworkAdvanced
            diagnostics={diagnostics}
            adapters={adapters}
            allAdapters={allAdapters}
            locked={locked}
            actionKey={actionKey}
            helperReason={helperReason}
            agentReason={agentReason}
            runAction={runAction}
            onFlushDnsCache={onFlushDnsCache}
            onResetWinsockCatalog={onResetWinsockCatalog}
            adapterBusyName={adapterBusyName}
            onToggleAdapter={(name, enabled) => void toggleAdapter(name, enabled)}
            dnsAdapterName={dnsAdapterName}
            onDnsAdapterChange={setDnsAdapterName}
            onApplyDns={applyDns}
            tuneBusy={tuneBusy}
            tuneError={tuneError}
            tuneSession={tunePending}
            onApplyTune={applyTune}
          />
        </AdvancedDisclosure>
      )}

      <p className="text-xs text-fg-3">{t("mypc.network.footer")}</p>

      {tunePending && !awaitingReboot && (
        <TuneDialog
          session={tunePending}
          secondsLeft={tuneSecondsLeft}
          busy={tuneBusy}
          onKeep={() => void confirmTune()}
          onUndo={() => void revertTune()}
          onRestart={() => void restartNow()}
        />
      )}
    </div>
  );
}

const TONE_DOT: Record<HealthTone, string> = { ok: "bg-ok", warn: "bg-warn", danger: "bg-danger" };
const TONE_BORDER: Record<HealthTone, string> = { ok: "border-ok/30", warn: "border-warn/40", danger: "border-danger-line" };

function StatusBand({
  diagnostics,
  tone,
  headlineCode,
  detailCodes,
  loading,
  refreshing,
  error,
  onRetest,
  children,
}: {
  diagnostics: NetworkDiagnostics | null;
  tone: HealthTone;
  headlineCode: string;
  detailCodes: string[];
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  onRetest: () => void;
  children?: ReactNode;
}) {
  const { t } = useI18n();
  const lessThanOne = t("mypc.network.lessThanOneMs");
  const noReading = <span className="font-normal text-fg-3">{t("mypc.network.band.noReading")}</span>;
  const failedWithoutData = !diagnostics && !loading && Boolean(error);

  const metric = (label: string, value: ReactNode) => (
    <span className="num whitespace-nowrap text-[13px] text-fg-3">
      {label} <span className="text-[17px] font-semibold text-fg">{value}</span>
    </span>
  );

  return (
    <section
      className={cn(
        "flex flex-col gap-3 rounded-card border bg-surface px-5 py-4",
        loading || failedWithoutData || !diagnostics ? "border-line" : TONE_BORDER[tone],
      )}
      aria-busy={loading || refreshing}
    >
      <div className="flex flex-wrap items-center gap-x-8 gap-y-2">
        {loading ? (
          <div className="flex min-w-0 flex-1 items-center gap-3" aria-live="polite">
            <Loader2 className="h-4 w-4 shrink-0 animate-spin text-accent" aria-hidden />
            <div>
              <div className="text-lg font-semibold text-fg">{t("mypc.network.state.measuring")}</div>
              <p className="text-[13px] text-fg-3">{t("mypc.network.state.measuringDesc")}</p>
            </div>
          </div>
        ) : failedWithoutData ? (
          <div className="min-w-0 flex-1">
            <div className="text-lg font-semibold text-fg">{t("mypc.network.state.failedTitle")}</div>
            <p className="text-[13px] text-danger">{error}</p>
          </div>
        ) : (
          <>
            <div className="flex items-center gap-2.5">
              <span className={cn("h-2.5 w-2.5 shrink-0 rounded-pill", diagnostics ? TONE_DOT[tone] : "bg-fg-4")} aria-hidden />
              <span className="text-lg font-semibold text-fg">{t(`mypc.network.state.${headlineCode}`)}</span>
            </div>
            {diagnostics && (
              <>
                {metric(t("mypc.network.band.internet"), diagnostics.external_latency_ms != null ? formatMs(diagnostics.external_latency_ms, lessThanOne) : noReading)}
                {metric(t("mypc.network.band.router"), diagnostics.gateway_latency_ms != null ? formatMs(diagnostics.gateway_latency_ms, lessThanOne) : noReading)}
                {metric(
                  t("mypc.network.band.loss"),
                  diagnostics.packet_loss_percent != null ? `${formatNumber(diagnostics.packet_loss_percent)}%` : noReading,
                )}
                {diagnostics.wifi_signal_percent != null ? (
                  <span className="num whitespace-nowrap text-[13px] text-fg-3">
                    {t("mypc.network.band.wifi")}{" "}
                    <span className="text-[17px] font-semibold text-fg">{formatPercent(diagnostics.wifi_signal_percent)}</span>
                    {diagnostics.link_speed && <span> · {diagnostics.link_speed}</span>}
                  </span>
                ) : (
                  diagnostics.link_speed && <span className="num whitespace-nowrap text-[13px] text-fg-3">{diagnostics.link_speed}</span>
                )}
              </>
            )}
          </>
        )}
        {!loading && (
          <Button size="sm" variant="outline" className="ml-auto" disabled={refreshing} onClick={onRetest}>
            {refreshing ? t("mypc.network.band.testing") : t("mypc.network.band.retest")}
          </Button>
        )}
      </div>
      {!loading && detailCodes.length > 0 && (
        <p className="text-[13px] text-fg-3">{detailCodes.map((code) => t(`mypc.network.state.${code}`)).join(" · ")}</p>
      )}
      {!loading && diagnostics && error && <p className="text-[13px] text-danger">{error}</p>}
      {children}
    </section>
  );
}

function RestartNotice({
  message,
  busy,
  onRestart,
  onDismiss,
}: {
  message: string;
  busy: boolean;
  onRestart: () => void;
  onDismiss?: () => void;
}) {
  const { t } = useI18n();
  return (
    <div role="status" className="flex flex-wrap items-center gap-3 rounded-card border border-warn/40 bg-warn/5 px-5 py-3">
      <span className="min-w-0 flex-1 text-[13px] text-warn">{message}</span>
      {onDismiss && (
        <Button size="sm" variant="link" onClick={onDismiss}>
          {t("mypc.network.advanced.tcp.close")}
        </Button>
      )}
      <Button size="sm" variant="outline" disabled={busy} onClick={onRestart}>
        {t("mypc.network.advanced.tcp.restartNow")}
      </Button>
    </div>
  );
}

function GameCard({
  diagnostics,
  busy,
  agentReason,
  onDiagnose,
}: {
  diagnostics: NetworkDiagnostics | null;
  busy: boolean;
  agentReason: string | null;
  onDiagnose: (ip: string) => void;
}) {
  const { t } = useI18n();
  const game = diagnostics?.game_server ?? null;
  const lessThanOne = t("mypc.network.lessThanOneMs");

  if (game?.latency_ms == null) {
    return (
      <Card label={t("mypc.network.game.label")}>
        <div className="text-[15px] font-semibold text-fg">{t("mypc.network.game.noneTitle")}</div>
        <p className="mt-1.5 text-[13px] leading-relaxed text-fg-3">{t("mypc.network.game.noneDesc")}</p>
      </Card>
    );
  }

  const high = isGameServerHigh(diagnostics);
  return (
    <Card label={t("mypc.network.game.label")}>
      <div className="truncate text-[15px] font-semibold text-fg">{game.process_name ?? t("mypc.network.game.unknownProcess")}</div>
      <div className="num truncate text-xs text-fg-3">
        {t("mypc.network.game.server")} {game.remote_ip}:{game.remote_port}
      </div>
      <div className="mt-3 flex items-center gap-2">
        <span className="num text-2xl font-semibold text-fg">{formatMs(game.latency_ms, lessThanOne)}</span>
        <StatusPill tone={high ? "warn" : "ok"}>{high ? t("mypc.network.game.high") : t("mypc.network.game.normal")}</StatusPill>
      </div>
      {diagnostics?.external_latency_ms != null && (
        <div className="num mt-0.5 text-[13px] text-fg-3">
          {t("mypc.network.game.vsInternet", { value: formatMs(diagnostics.external_latency_ms, lessThanOne) })}
        </div>
      )}
      {high && <p className="mt-2 text-[13px] text-warn">{t("mypc.network.game.highDesc")}</p>}
      {game.best_guess && <p className="mt-2 text-xs text-fg-3">{t("mypc.network.game.bestGuess")}</p>}
      <Button
        className="mt-3"
        size="sm"
        variant={high ? "primary" : "outline"}
        disabled={busy}
        disabledReason={agentReason}
        onClick={() => onDiagnose(game.remote_ip)}
      >
        {t("mypc.network.game.diagnose")}
      </Button>
    </Card>
  );
}

function PathCard({
  diagnostics,
  trace,
  verdict,
  onOpenRoute,
}: {
  diagnostics: NetworkDiagnostics | null;
  trace: TracerouteResult | null;
  verdict: RouteVerdict | null;
  onOpenRoute: () => void;
}) {
  const { t } = useI18n();
  const lessThanOne = t("mypc.network.lessThanOneMs");
  const gateway = diagnostics?.gateway_latency_ms ?? null;
  const internet = diagnostics?.external_latency_ms ?? null;
  const rows: Array<[string, number]> = [];
  if (gateway != null) rows.push([t("mypc.network.path.pcToRouter"), gateway]);
  if (gateway != null && internet != null) rows.push([t("mypc.network.path.routerToInternet"), Math.max(0, internet - gateway)]);

  return (
    <Card label={t("mypc.network.path.label")}>
      {rows.length === 0 ? (
        <EmptyState variant="inline" title={t("mypc.network.path.noData")} reason={t("mypc.network.path.noDataReason")} />
      ) : (
        <ul className="flex flex-col gap-1.5">
          {rows.map(([label, value]) => (
            <li key={label} className="num text-[13px] text-fg-2">
              {label} <span className="font-semibold text-fg">{formatMs(value, lessThanOne)}</span>
            </li>
          ))}
        </ul>
      )}
      {trace && verdict && (
        <div className="mt-3 border-t border-line-soft pt-3 text-[13px]">
          <span className="text-fg-3">{t("mypc.network.path.lastTrace", { target: trace.target })}</span>{" "}
          <span className="text-fg">{t(`mypc.network.route.verdict.${verdict.kind}Title`)}</span>
          {verdict.culpritHop != null && (
            <StatusPill tone={verdictTone(verdict.kind)} className="ml-2">
              {t("mypc.network.route.culprit", { hop: verdict.culpritHop })}
            </StatusPill>
          )}
        </div>
      )}
      <Button className="mt-3" size="sm" variant="link" onClick={onOpenRoute}>
        {t("mypc.network.path.full")}
      </Button>
    </Card>
  );
}

function DnsCard({
  results,
  busy,
  error,
  suggestion,
  locked,
  actionKey,
  agentReason,
  helperReason,
  onRun,
  onUse,
  onApplySuggestion,
  onDismissSuggestion,
}: {
  results: DnsBenchmarkResult[] | null;
  busy: boolean;
  error: string | null;
  suggestion: DnsOptimizationSuggestion | null;
  locked: boolean;
  actionKey: string | null;
  agentReason: string | null;
  helperReason: string | null;
  onRun: () => void;
  onUse: (server: string) => void;
  onApplySuggestion: () => void;
  onDismissSuggestion: () => void;
}) {
  const { t } = useI18n();
  const lessThanOne = t("mypc.network.lessThanOneMs");
  const sorted = results ? [...results].sort((a, b) => (a.latencyMs ?? Infinity) - (b.latencyMs ?? Infinity)) : null;
  const fastest = sorted?.find((item) => item.latencyMs != null) ?? null;
  const latencyText = (value: number | null | undefined) =>
    value == null ? t("mypc.network.dns.noAnswer") : formatMs(value, lessThanOne);

  return (
    <Card label={t("mypc.network.dns.label")}>
      {suggestion && (
        <div className="mb-3 rounded-control border border-line bg-surface-2 p-3">
          <div className="text-sm font-semibold text-fg">{t("mypc.network.dns.suggestionTitle", { server: suggestion.dnsLabel })}</div>
          <p className="num mt-0.5 text-xs text-fg-3">
            {t("mypc.network.dns.suggestionDesc", {
              server: suggestion.dnsLabel,
              previous: latencyText(suggestion.previousLatencyMs),
              candidate: latencyText(suggestion.candidateLatencyMs),
            })}
          </p>
          <div className="mt-2 flex items-center gap-3">
            <Button
              size="sm"
              variant="outline"
              disabled={locked}
              disabledReason={agentReason ?? helperReason}
              onClick={onApplySuggestion}
            >
              {actionKey === "dnsSuggestion" ? t("mypc.network.working") : t("mypc.network.dns.suggestionApply", { server: suggestion.dnsLabel })}
            </Button>
            <Button size="sm" variant="link" onClick={onDismissSuggestion}>
              {t("mypc.network.dns.dismiss")}
            </Button>
          </div>
        </div>
      )}

      {!sorted ? (
        busy ? (
          <p className="flex items-center gap-2 text-[13px] text-fg-3" aria-live="polite">
            <Loader2 className="h-4 w-4 animate-spin text-accent" aria-hidden />
            {t("mypc.network.dns.testing")}
          </p>
        ) : (
          <EmptyState
            variant="inline"
            className="flex-col items-start"
            title={<span className="font-semibold text-fg">{t("mypc.network.dns.notTested")}</span>}
            reason={t("mypc.network.dns.notTestedReason")}
            action={
              <Button size="sm" variant="link" disabledReason={agentReason} onClick={onRun}>
                {t("mypc.network.dns.test")}
              </Button>
            }
          />
        )
      ) : (
        <>
          <div className="text-[15px] font-semibold text-fg">
            {!fastest || fastest.isCurrent
              ? t("mypc.network.dns.currentFastest")
              : t("mypc.network.dns.fasterFound", { label: fastest.label })}
          </div>
          <ul className="mt-2">
            {sorted.map((item) => (
              <li key={item.server} className="num flex items-center gap-2 border-b border-line-soft py-1.5 text-[13px] last:border-b-0">
                <span className="min-w-0 flex-1 truncate text-fg-2" title={item.server}>
                  {item.label}
                  {item.isCurrent && <span className="ml-1.5 text-xs text-accent">{t("mypc.network.dns.current")}</span>}
                </span>
                <span className="shrink-0 text-fg-3">{latencyText(item.latencyMs)}</span>
                {!item.isCurrent && item.latencyMs != null && (
                  <Button
                    size="sm"
                    variant="link"
                    className="text-xs"
                    disabled={locked}
                    disabledReason={agentReason ?? helperReason}
                    onClick={() => onUse(item.server)}
                  >
                    {actionKey === `dns:${item.server}` ? t("mypc.network.working") : t("mypc.network.dns.useThis", { label: item.label })}
                  </Button>
                )}
              </li>
            ))}
          </ul>
          {fastest && !fastest.isCurrent && <p className="mt-2 text-xs text-fg-3">{t("mypc.network.dns.youDecide")}</p>}
          <Button className="mt-2" size="sm" variant="link" disabled={busy} disabledReason={agentReason} onClick={onRun}>
            {busy ? t("mypc.network.dns.testing") : t("mypc.network.dns.retest")}
          </Button>
        </>
      )}
      {error && <p className="mt-2 text-[13px] text-danger">{error}</p>}
    </Card>
  );
}
