import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, FileWarning, Gauge, Radio, RefreshCw, Route, Router, Video, Wifi } from "lucide-react";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  benchmarkDnsServers,
  detectLiveModeStreamingApp,
  generateLiveModeIncidentReport,
  getLiveModeStatus,
  getNetworkDiagnostics,
  getPrivilegedHelperStatus,
  isTauriRuntime,
  listNetworkAdapters,
  listenToLiveModeIncident,
  listenToLiveModeSample,
  probeGatewayIdentity,
  runNetworkTraceroute,
  startLiveMode,
  stopLiveMode,
  type BitrateRecommendation,
  type DnsBenchmarkResult,
  type IncidentReport,
  type LiveModeSample,
  type NetworkAdapterSummary,
  type NetworkDiagnostics,
  type PrivilegedHelperStatus,
  type TracerouteResult,
} from "@/services/tauri/agent";

const LIVE_MODE_SAMPLE_HISTORY = 60;
const STREAMING_APP_POLL_MS = 15_000;

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Consolidated network home - diagnostics + admin actions (DNS/Winsock)
 * that used to live buried in Controles > Avancado, plus Modo Live (was in
 * Telemetria). One place for "everything about the network", with room to
 * grow into future buffer/cache controls without spreading across more
 * screens. */
export function Network({
  busy,
  isReady,
  onFlushDnsCache,
  onSetDnsServers,
  onResetWinsockCatalog,
}: {
  busy: boolean;
  isReady: boolean;
  onFlushDnsCache: () => Promise<unknown>;
  onSetDnsServers: (adapterName: string, dnsServers: string[]) => Promise<unknown>;
  onResetWinsockCatalog: () => Promise<unknown>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("network");
  const runtimeAvailable = isTauriRuntime();

  const [networkDiagnostics, setNetworkDiagnostics] = useState<NetworkDiagnostics | null>(null);
  const [networkAdapters, setNetworkAdapters] = useState<NetworkAdapterSummary[]>([]);
  const [dnsAdapterName, setDnsAdapterName] = useState("");
  const [dnsPrimary, setDnsPrimary] = useState("");
  const [dnsSecondary, setDnsSecondary] = useState("");
  const [helperStatus, setHelperStatus] = useState<PrivilegedHelperStatus | null>(null);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [diagnosticsError, setDiagnosticsError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [gatewayIdentity, setGatewayIdentity] = useState<string | null>(null);
  const [gatewayIdentityBusy, setGatewayIdentityBusy] = useState(false);

  const [tracerouteTarget, setTracerouteTarget] = useState("1.1.1.1");
  const [tracerouteResult, setTracerouteResult] = useState<TracerouteResult | null>(null);
  const [tracerouteBusy, setTracerouteBusy] = useState(false);
  const [tracerouteError, setTracerouteError] = useState<string | null>(null);

  const [dnsBenchmarkResults, setDnsBenchmarkResults] = useState<DnsBenchmarkResult[] | null>(null);
  const [dnsBenchmarkBusy, setDnsBenchmarkBusy] = useState(false);
  const [dnsBenchmarkError, setDnsBenchmarkError] = useState<string | null>(null);

  const [liveModeActive, setLiveModeActive] = useState(false);
  const [liveModeBusy, setLiveModeBusy] = useState(false);
  const [liveModeSamples, setLiveModeSamples] = useState<LiveModeSample[]>([]);
  const [streamingAppDetected, setStreamingAppDetected] = useState<string | null>(null);
  const [bitrateRecommendation, setBitrateRecommendation] = useState<BitrateRecommendation | null>(null);
  const [lastIncident, setLastIncident] = useState<IncidentReport | null>(null);
  const [incidentReportBusy, setIncidentReportBusy] = useState(false);
  const latestLiveSample = liveModeSamples[liveModeSamples.length - 1] ?? null;

  const refreshNetwork = useCallback(async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsError(null);
    try {
      const [nextNetwork, nextAdapters, nextHelper] = await Promise.all([
        getNetworkDiagnostics(),
        listNetworkAdapters(),
        getPrivilegedHelperStatus(),
      ]);
      setNetworkDiagnostics(nextNetwork);
      setNetworkAdapters(nextAdapters);
      setHelperStatus(nextHelper);
      setDnsAdapterName((current) => {
        if (current && nextAdapters.some((adapter) => adapter.name === current)) return current;
        return nextNetwork.adapter_name ?? nextAdapters[0]?.name ?? "";
      });
      track("network_refreshed");
    } catch (error) {
      setNetworkDiagnostics(null);
      setNetworkAdapters([]);
      setDiagnosticsError(errorMessage(error));
    } finally {
      setDiagnosticsBusy(false);
    }
  }, [track]);

  useEffect(() => {
    void refreshNetwork();
  }, [refreshNetwork]);

  useEffect(() => {
    if (!isReady) return;
    let canceled = false;
    const poll = () => {
      detectLiveModeStreamingApp()
        .then((app) => {
          if (!canceled) setStreamingAppDetected(app);
        })
        .catch(() => undefined);
    };
    poll();
    const id = window.setInterval(poll, STREAMING_APP_POLL_MS);
    return () => {
      canceled = true;
      window.clearInterval(id);
    };
  }, [isReady]);

  useEffect(() => {
    if (!isReady) return;
    getLiveModeStatus()
      .then((status) => {
        setLiveModeActive(status.active);
        setLiveModeSamples(status.samples.slice(-LIVE_MODE_SAMPLE_HISTORY));
        setBitrateRecommendation(status.bitrateRecommendation ?? null);
        setLastIncident(status.lastIncident ?? null);
      })
      .catch(() => undefined);
  }, [isReady]);

  useEffect(() => {
    let disposeSample: (() => void) | undefined;
    let disposeIncident: (() => void) | undefined;
    listenToLiveModeSample((liveSample) => {
      setLiveModeSamples((current) => [...current.slice(-(LIVE_MODE_SAMPLE_HISTORY - 1)), liveSample]);
    }).then((dispose) => {
      disposeSample = dispose;
    });
    listenToLiveModeIncident((report) => {
      setLastIncident(report);
    }).then((dispose) => {
      disposeIncident = dispose;
    });
    return () => {
      disposeSample?.();
      disposeIncident?.();
    };
  }, []);

  useEffect(() => {
    if (!liveModeActive) return;
    const id = window.setInterval(() => {
      getLiveModeStatus()
        .then((status) => {
          setBitrateRecommendation(status.bitrateRecommendation ?? null);
          setLastIncident(status.lastIncident ?? null);
        })
        .catch(() => undefined);
    }, 5_000);
    return () => window.clearInterval(id);
  }, [liveModeActive]);

  const toggleLiveMode = useCallback(async () => {
    if (!isReady || liveModeBusy) return;
    setLiveModeBusy(true);
    try {
      if (liveModeActive) {
        await stopLiveMode();
        setLiveModeActive(false);
      } else {
        setLiveModeSamples([]);
        await startLiveMode();
        setLiveModeActive(true);
      }
      track("live_mode_toggled", { enabled: !liveModeActive });
    } catch {
      track("live_mode_toggle_failed");
    } finally {
      setLiveModeBusy(false);
    }
  }, [isReady, liveModeActive, liveModeBusy, track]);

  const generateIncidentReport = useCallback(async () => {
    if (incidentReportBusy) return;
    setIncidentReportBusy(true);
    try {
      const report = await generateLiveModeIncidentReport();
      setLastIncident(report);
      track("live_mode_incident_report_generated");
    } catch {
      track("live_mode_incident_report_failed");
    } finally {
      setIncidentReportBusy(false);
    }
  }, [incidentReportBusy, track]);

  const runNetworkAction = async (action: () => Promise<unknown>, successMessage: string) => {
    setActionMessage(null);
    try {
      const result = await action();
      if (result === false) {
        setActionMessage(t("controls.actionCancelled"));
        return;
      }
      setActionMessage(successMessage);
      await refreshNetwork();
    } catch (error) {
      setActionMessage(errorMessage(error));
    }
  };

  const applyDnsServers = async () => {
    const servers = [dnsPrimary, dnsSecondary].map((value) => value.trim()).filter(Boolean);
    if (!dnsAdapterName || servers.length === 0) {
      setActionMessage("Selecione um adaptador e informe ao menos um servidor DNS.");
      return;
    }
    await runNetworkAction(
      () => onSetDnsServers(dnsAdapterName, servers),
      "Servidores DNS alterados.",
    );
  };

  const identifyGateway = async () => {
    if (!networkDiagnostics?.gateway) return;
    setGatewayIdentityBusy(true);
    try {
      const identity = await probeGatewayIdentity(networkDiagnostics.gateway);
      setGatewayIdentity(identity ?? t("network.gatewayIdentityUnavailable"));
      track("gateway_identity_probed", { found: Boolean(identity) });
    } catch (error) {
      setGatewayIdentity(t("network.gatewayIdentityUnavailable"));
    } finally {
      setGatewayIdentityBusy(false);
    }
  };

  const runTraceroute = async () => {
    setTracerouteBusy(true);
    setTracerouteError(null);
    try {
      const result = await runNetworkTraceroute(tracerouteTarget.trim() || undefined);
      setTracerouteResult(result);
      track("traceroute_run", { target: result.target, hops: result.hops.length });
    } catch (error) {
      setTracerouteError(errorMessage(error));
    } finally {
      setTracerouteBusy(false);
    }
  };

  const runDnsBenchmark = async () => {
    setDnsBenchmarkBusy(true);
    setDnsBenchmarkError(null);
    try {
      const results = await benchmarkDnsServers(networkDiagnostics?.dns_servers ?? []);
      setDnsBenchmarkResults(results);
      track("dns_benchmark_run", { count: results.length });
    } catch (error) {
      setDnsBenchmarkError(errorMessage(error));
    } finally {
      setDnsBenchmarkBusy(false);
    }
  };

  const useBenchmarkedDns = async (server: string) => {
    if (!dnsAdapterName) {
      setActionMessage(t("network.selectAdapterFirst"));
      return;
    }
    await runNetworkAction(
      () => onSetDnsServers(dnsAdapterName, [server]),
      "Servidores DNS alterados.",
    );
  };

  return (
    <div className="flex flex-col gap-8">
      <header className="flex flex-col gap-2">
        <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
          <Wifi className="h-3 w-3" />
          {t("network.eyebrow")}
        </div>
        <h1 className="text-[36px] font-semibold tracking-tight text-slate-50">{t("network.title")}</h1>
      </header>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Wifi className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("network.diagnostics")}</h2>
          </div>
          <button
            disabled={diagnosticsBusy}
            onClick={() => void refreshNetwork()}
            className="inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${diagnosticsBusy ? "animate-spin" : ""}`} />
            {t("common.refresh")}
          </button>
        </div>
        {diagnosticsError && <Notice tone="danger" message={diagnosticsError} />}
        {actionMessage && <div className="mb-4"><Notice tone="info" message={actionMessage} /></div>}

        <div className="grid gap-2 sm:grid-cols-2">
          {([
            ["Status", networkDiagnostics ? (networkDiagnostics.connected ? "online" : "offline") : "--"],
            ["Adaptador", networkDiagnostics?.adapter_name ?? "--"],
            ["Tipo", networkDiagnostics?.adapter_type ?? networkDiagnostics?.adapter_description ?? "--"],
            ["Link", networkDiagnostics?.link_speed ?? "--"],
            ["Wi-Fi", networkDiagnostics?.wifi_ssid ? `${networkDiagnostics.wifi_ssid}${networkDiagnostics.wifi_signal_percent != null ? ` - ${Math.round(networkDiagnostics.wifi_signal_percent)}%` : ""}` : "--"],
            [
              t("network.gateway"),
              networkDiagnostics?.gateway
                ? `${networkDiagnostics.gateway}${networkDiagnostics.gateway_latency_ms != null ? ` - ${Math.round(networkDiagnostics.gateway_latency_ms)} ms` : ""}`
                : "--",
            ],
          ] as Array<[string, string]>).map(([label, value]) => (
            <div key={label} className="rounded-lg border border-cyan-500/10 bg-slate-950/50 p-3">
              <div className="font-mono text-[9px] uppercase tracking-widest text-slate-500">{label}</div>
              <div className="mt-1 truncate text-sm font-semibold text-slate-100" title={value}>
                {value}
              </div>
            </div>
          ))}
        </div>
        <p className="mt-3 text-xs text-slate-500">
          {t("network.diagnosticsFooter")} {networkDiagnostics?.recommendations?.join(" / ") ?? ""}
        </p>

        {networkDiagnostics?.gateway && (
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              disabled={gatewayIdentityBusy}
              onClick={() => void identifyGateway()}
              className="inline-flex items-center gap-2 rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              <Router className={`h-3.5 w-3.5 ${gatewayIdentityBusy ? "animate-pulse" : ""}`} />
              {t("network.identifyGateway")}
            </button>
            {gatewayIdentity && (
              <span className="text-xs text-slate-400" title={gatewayIdentity}>
                {gatewayIdentity}
              </span>
            )}
          </div>
        )}

        <div className="mt-5 rounded-xl border border-cyan-500/10 bg-slate-950/40 p-4">
          <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
            <Wifi className="h-4 w-4 text-cyan-300" />
            {t("network.adminActions")}
          </div>
          <p className="mt-1 text-xs text-slate-500">{t("network.adminActionsDesc")}</p>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <button
              disabled={busy || diagnosticsBusy || !runtimeAvailable}
              onClick={() => void runNetworkAction(onFlushDnsCache, "Cache de DNS limpo.")}
              className="rounded-lg border border-cyan-400/30 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              Limpar cache DNS
            </button>
            <button
              disabled={busy || diagnosticsBusy || !runtimeAvailable || !helperStatus?.available}
              onClick={() => void runNetworkAction(
                onResetWinsockCatalog,
                "Catalogo Winsock resetado. Reinicie o computador para concluir.",
              )}
              className="rounded-lg border border-rose-400/30 bg-rose-400/10 px-3 py-2 text-xs font-medium text-rose-100 transition hover:bg-rose-400/15 disabled:opacity-50"
              title={!helperStatus?.available ? "Instale o helper privilegiado (Controles > Avancado) para liberar esta acao." : "Exige reinicializacao do computador."}
            >
              Resetar Winsock
            </button>
          </div>

          <div className="mt-4 grid gap-2 sm:grid-cols-4">
            <select
              value={dnsAdapterName}
              onChange={(event) => setDnsAdapterName(event.target.value)}
              disabled={busy || diagnosticsBusy || !runtimeAvailable || networkAdapters.length === 0}
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60 sm:col-span-2 disabled:opacity-50"
            >
              {networkAdapters.length === 0 ? (
                <option value="">{t("controls.noActiveAdapter")}</option>
              ) : (
                networkAdapters.map((adapter) => (
                  <option key={adapter.name} value={adapter.name}>
                    {adapter.name}
                  </option>
                ))
              )}
            </select>
            <input
              value={dnsPrimary}
              onChange={(event) => setDnsPrimary(event.target.value)}
              placeholder="DNS primario (ex: 1.1.1.1)"
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60"
            />
            <input
              value={dnsSecondary}
              onChange={(event) => setDnsSecondary(event.target.value)}
              placeholder="DNS secundario (opcional)"
              className="min-h-11 rounded-xl border border-cyan-500/20 bg-slate-950/60 px-3 text-sm text-slate-100 outline-none transition focus:border-cyan-300/60"
            />
          </div>
          <button
            disabled={busy || diagnosticsBusy || !runtimeAvailable || !helperStatus?.available || !dnsAdapterName}
            onClick={() => void applyDnsServers()}
            className="mt-3 inline-flex items-center gap-2 rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2.5 text-sm font-semibold text-cyan-100 transition-all hover:bg-cyan-400/15 disabled:opacity-50"
            title={!helperStatus?.available ? "Instale o helper privilegiado (Controles > Avancado) para liberar esta acao." : undefined}
          >
            Aplicar DNS
          </button>
        </div>
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Route className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("network.route")}</h2>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input
              value={tracerouteTarget}
              onChange={(event) => setTracerouteTarget(event.target.value)}
              placeholder="1.1.1.1"
              className="min-h-9 w-40 rounded-lg border border-cyan-500/20 bg-slate-950/60 px-3 text-xs text-slate-100 outline-none transition focus:border-cyan-300/60"
            />
            <button
              disabled={tracerouteBusy || !runtimeAvailable}
              onClick={() => void runTraceroute()}
              className="inline-flex items-center gap-2 rounded-lg border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${tracerouteBusy ? "animate-spin" : ""}`} />
              {tracerouteBusy ? t("network.tracing") : t("network.traceRoute")}
            </button>
          </div>
        </div>
        <p className="text-xs text-slate-500">{t("network.routeDesc")}</p>
        {tracerouteError && <div className="mt-3"><Notice tone="danger" message={tracerouteError} /></div>}

        {tracerouteResult && (
          <div className="mt-4 flex flex-col divide-y divide-cyan-500/5 overflow-hidden rounded-xl border border-cyan-500/10">
            {tracerouteResult.hops.map((hop) => (
              <div key={hop.hop} className="flex items-center gap-3 bg-slate-950/30 px-3 py-2 text-xs">
                <span className="w-6 shrink-0 text-right font-mono text-slate-500">{hop.hop}</span>
                <span className={`min-w-0 flex-1 truncate font-mono ${hop.timedOut ? "text-slate-600" : "text-slate-200"}`}>
                  {hop.timedOut ? t("network.hopTimedOut") : hop.ip}
                  {!hop.timedOut && hop.ip === tracerouteResult.target && (
                    <span className="ml-2 rounded border border-emerald-400/30 bg-emerald-400/10 px-1.5 py-0.5 text-[9px] uppercase tracking-widest text-emerald-300">
                      {t("network.destination")}
                    </span>
                  )}
                </span>
                <span className="flex shrink-0 gap-2 font-mono text-slate-400">
                  {hop.rttMs.map((value, index) => (
                    <span key={index} className="w-12 text-right">
                      {value == null ? "*" : `${Math.round(value)}ms`}
                    </span>
                  ))}
                </span>
              </div>
            ))}
            {!tracerouteResult.reachedTarget && (
              <p className="bg-slate-950/30 px-3 py-2 text-xs text-amber-300">{t("network.routeIncomplete")}</p>
            )}
          </div>
        )}
      </section>

      <section className="glass-panel cyber-glow p-6">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Gauge className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">{t("network.dnsBenchmark")}</h2>
          </div>
          <button
            disabled={dnsBenchmarkBusy || !runtimeAvailable}
            onClick={() => void runDnsBenchmark()}
            className="inline-flex items-center gap-2 rounded-lg border border-cyan-400/40 bg-cyan-400/10 px-3 py-2 text-xs font-medium text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${dnsBenchmarkBusy ? "animate-spin" : ""}`} />
            {dnsBenchmarkBusy ? t("network.testing") : t("network.testDns")}
          </button>
        </div>
        <p className="text-xs text-slate-500">{t("network.dnsBenchmarkDesc")}</p>
        {dnsBenchmarkError && <div className="mt-3"><Notice tone="danger" message={dnsBenchmarkError} /></div>}

        {dnsBenchmarkResults && (
          <div className="mt-4 flex flex-col divide-y divide-cyan-500/5 overflow-hidden rounded-xl border border-cyan-500/10">
            {[...dnsBenchmarkResults]
              .sort((a, b) => (a.latencyMs ?? Infinity) - (b.latencyMs ?? Infinity))
              .map((result, index) => (
                <div key={result.server} className="flex items-center gap-3 bg-slate-950/30 px-3 py-2.5 text-sm">
                  {index === 0 && result.latencyMs != null && (
                    <span className="shrink-0 rounded-md border border-emerald-400/30 bg-emerald-400/10 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-widest text-emerald-300">
                      {t("network.fastest")}
                    </span>
                  )}
                  <span className="min-w-0 flex-1 truncate text-slate-100">
                    {result.label} <span className="text-slate-500">({result.server})</span>
                    {result.isCurrent && (
                      <span className="ml-2 text-[10px] uppercase tracking-widest text-cyan-400">{t("network.currentDns")}</span>
                    )}
                  </span>
                  <span className="w-16 shrink-0 text-right font-mono text-xs text-slate-400">
                    {result.latencyMs == null ? "--" : `${Math.round(result.latencyMs)} ms`}
                  </span>
                  {!result.isCurrent && (
                    <button
                      disabled={busy || diagnosticsBusy || !runtimeAvailable || !helperStatus?.available}
                      onClick={() => void useBenchmarkedDns(result.server)}
                      className="shrink-0 rounded-md border border-cyan-400/30 bg-cyan-400/10 px-2 py-1 font-mono text-[10px] uppercase tracking-widest text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
                      title={!helperStatus?.available ? "Instale o helper privilegiado (Controles > Avancado) para liberar esta acao." : undefined}
                    >
                      {t("network.useThis")}
                    </button>
                  )}
                </div>
              ))}
          </div>
        )}
      </section>

      <section className="glass-panel cyber-glow p-5">
        <div className="flex flex-col gap-3 pb-4 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-2">
            <Video className="h-3.5 w-3.5 text-cyan-300" />
            <h2 className="font-mono text-[11px] uppercase tracking-[0.25em] text-cyan-400/80">Modo Live</h2>
          </div>
          <button
            role="switch"
            aria-checked={liveModeActive}
            disabled={!isReady || liveModeBusy}
            onClick={() => void toggleLiveMode()}
            className={`inline-flex min-h-11 items-center gap-2 rounded-xl border px-4 py-2 text-sm font-semibold transition-all disabled:opacity-50 ${
              liveModeActive
                ? "border-emerald-300/50 bg-emerald-400/15 text-emerald-100"
                : "border-cyan-400/40 bg-cyan-400/10 text-cyan-100 hover:border-cyan-300/60"
            }`}
          >
            <Radio className="h-4 w-4" />
            {liveModeActive ? "Desativar Modo Live" : "Ativar Modo Live"}
          </button>
        </div>

        {streamingAppDetected && !liveModeActive && (
          <div className="mb-3 flex items-center gap-3 rounded-xl border border-cyan-400/20 bg-cyan-400/5 px-4 py-3 text-sm text-cyan-100">
            <Video className="h-4 w-4 shrink-0" />
            Detectamos {streamingAppDetected} em primeiro plano. O Modo Live amostra a rede com mais frequencia e avisa de instabilidades durante a transmissao.
          </div>
        )}

        {!liveModeActive && !streamingAppDetected && (
          <p className="text-sm text-slate-400">
            Ativacao manual: amostra a rede com mais frequencia durante uma transmissao e gera recomendacao de
            bitrate e relatorio de incidentes. O AnalystBlaze nao tem integracao com OBS/Streamlabs/etc. - so observa
            a rede local, nunca aplica nada no seu software de transmissao.
          </p>
        )}

        {liveModeActive && (
          <>
            <div className="grid gap-3 sm:grid-cols-3">
              <MiniStat label="ping" value={formatLiveMetric(latestLiveSample?.pingMs, "ms")} />
              <MiniStat label="jitter" value={formatLiveMetric(latestLiveSample?.jitterMs, "ms")} />
              <MiniStat label="perda" value={formatLiveMetric(latestLiveSample?.packetLossPercent, "%")} />
            </div>

            {bitrateRecommendation && (
              <div className="mt-3 rounded-xl border border-cyan-500/10 bg-slate-950/45 p-4">
                <div className="flex items-center justify-between gap-2">
                  <div className="font-mono text-[10px] uppercase tracking-widest text-slate-500">
                    Recomendacao de bitrate (estimativa local)
                  </div>
                  <span className="font-mono text-[10px] text-slate-500">
                    {Math.round(bitrateRecommendation.confidence * 100)}% confianca
                  </span>
                </div>
                <div className="mt-1 text-2xl font-semibold tracking-tight text-slate-50 tabular-nums">
                  {bitrateRecommendation.recommendedKbps} kbps
                </div>
                <p className="mt-1 text-xs leading-relaxed text-slate-500">{bitrateRecommendation.reason}</p>
              </div>
            )}

            <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
              <span className="text-xs text-slate-500">{liveModeSamples.length} amostras nesta sessao</span>
              <button
                disabled={incidentReportBusy}
                onClick={() => void generateIncidentReport()}
                className="inline-flex items-center gap-2 rounded-xl border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-xs font-medium text-amber-100 transition-all hover:border-amber-300/50 disabled:opacity-50"
              >
                <FileWarning className="h-3.5 w-3.5" />
                Gerar relatorio de incidente
              </button>
            </div>

            {lastIncident && (
              <div className="mt-3 rounded-xl border border-amber-400/15 bg-amber-400/5 p-4">
                <div className="mb-2 flex items-center gap-2 font-mono text-[10px] uppercase tracking-widest text-amber-200">
                  <AlertTriangle className="h-3.5 w-3.5" />
                  Causas provaveis (baseado em {lastIncident.sampleCount} amostras)
                </div>
                <ul className="flex flex-col gap-2">
                  {lastIncident.causes.map((cause) => (
                    <li key={cause.label} className="text-xs leading-relaxed text-amber-100/85">
                      <span className="font-semibold text-amber-100">{cause.label}</span>
                      {" "}
                      <span className="font-mono text-amber-200/70">({Math.round(cause.confidence * 100)}%)</span>
                      {" - "}
                      {cause.evidence}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </>
        )}
      </section>
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-cyan-500/10 bg-slate-950/45 px-3 py-2">
      <div className="font-mono text-[9px] uppercase tracking-widest text-slate-600">{label}</div>
      <div className="mt-0.5 text-sm font-semibold text-slate-200 tabular-nums">{value}</div>
    </div>
  );
}

function formatLiveMetric(value: number | null | undefined, unit: "ms" | "%") {
  if (value == null || !Number.isFinite(value)) return "--";
  return `${Math.round(value)} ${unit}`;
}

function Notice({ message, tone }: { message: string; tone: "danger" | "info" | "warning" }) {
  const toneClass =
    tone === "danger"
      ? "border-rose-400/25 bg-rose-400/10 text-rose-100"
      : tone === "warning"
        ? "border-amber-400/25 bg-amber-400/10 text-amber-100"
        : "border-cyan-400/25 bg-cyan-400/10 text-cyan-100";
  return (
    <div className={`rounded-xl border px-4 py-3 text-sm ${toneClass}`}>
      {message}
    </div>
  );
}
