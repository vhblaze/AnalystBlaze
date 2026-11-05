import { useCallback, useEffect, useMemo, useState } from "react";
import { Gauge, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Progress } from "@/components/ui/progress";
import { SectionLabel } from "@/components/ui/section-label";
import { Segmented } from "@/components/ui/segmented";
import { StatusPill } from "@/components/ui/status-pill";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { cssVar } from "@/design/tokens";
import { useTelemetry } from "@/hooks/useTelemetry";
import { toast } from "@/hooks/use-toast";
import { errorText } from "@/features/shared/agentResource";
import { useLiveTelemetry } from "@/features/telemetry/useLiveTelemetry";
import {
  readTelemetryValues,
  recordTelemetrySample,
  useTelemetryHistory,
  type TelemetryMetricKey,
  type TelemetryPoint,
} from "@/features/telemetry/telemetryHistory";
import { temperatureSourceLabel } from "@/lib/telemetry-labels";
import {
  displayProcessName,
  formatDuration,
  formatGhz,
  formatGigabytes,
  formatMegabytes,
  formatMs,
  formatNumber,
  formatPercent,
  formatTemperature,
  shortCpuName,
} from "@/lib/format";
import type { AgentTelemetrySample } from "@/services/tauri/agent";
import { mypc as ptMypc } from "@/i18n/pt/mypc";

/**
 * Meu PC › Desempenho (board Tela-MeuPC): a telemetria completa com a pele D3.
 * Mesmas props e funções de views/Telemetry.tsx; o histórico de 1 min / 10 min /
 * 1 hora vem de features/telemetry/telemetryHistory (módulo, sobrevive à troca
 * de aba). Sem "mais pesados agora": o agente não mede memória por processo (L1).
 */

type TFn = ReturnType<typeof useI18n>["t"];
type WindowKey = "1m" | "10m" | "1h";
type BarTone = "accent" | "ok" | "warn" | "danger";
type Status = "good" | "watch" | "critical";

type MetricDef = {
  key: TelemetryMetricKey;
  kind: "percent" | "ms" | "temp";
  /** Topo da escala do gráfico e da barra (cresce se a leitura passar). */
  scaleMax: number;
  tone: BarTone;
  status: (value: number) => Status;
};

const percentStatus = (value: number): Status => (value >= 90 ? "critical" : value >= 75 ? "watch" : "good");
const tempStatus = (value: number): Status => (value >= 88 ? "critical" : value >= 78 ? "watch" : "good");

const METRICS: MetricDef[] = [
  { key: "cpu", kind: "percent", scaleMax: 100, tone: "accent", status: percentStatus },
  { key: "gpu", kind: "percent", scaleMax: 100, tone: "accent", status: percentStatus },
  {
    key: "ram",
    kind: "percent",
    scaleMax: 100,
    tone: "warn",
    status: (v) => (v >= 92 ? "critical" : v >= 82 ? "watch" : "good"),
  },
  {
    key: "disk",
    kind: "percent",
    scaleMax: 100,
    tone: "accent",
    status: (v) => (v >= 95 ? "critical" : v >= 88 ? "watch" : "good"),
  },
  {
    key: "latency",
    kind: "ms",
    scaleMax: 150,
    tone: "ok",
    status: (v) => (v >= 110 ? "critical" : v >= 70 ? "watch" : "good"),
  },
  { key: "gpuTemp", kind: "temp", scaleMax: 100, tone: "danger", status: tempStatus },
  { key: "vram", kind: "percent", scaleMax: 100, tone: "accent", status: percentStatus },
  { key: "cpuTemp", kind: "temp", scaleMax: 100, tone: "warn", status: tempStatus },
];

const WINDOWS: Record<WindowKey, { seconds: number; bucket: number }> = {
  "1m": { seconds: 60, bucket: 1 },
  "10m": { seconds: 600, bucket: 3 },
  "1h": { seconds: 3600, bucket: 15 },
};

function formatValue(metric: MetricDef, value: number): string {
  if (metric.kind === "ms") return formatMs(value, "0 ms");
  if (metric.kind === "temp") return formatTemperature(value, true, "0 °C");
  return formatPercent(value, "0%");
}

function barPercent(metric: MetricDef, value: number): number {
  if (metric.kind === "percent") return value;
  return (value / metric.scaleMax) * 100;
}

export function Performance({
  latestSample,
  agentMode,
  isReady,
  busy,
  onCollectSample,
  onSetTelemetryMode,
  onOpenDiskUsage,
  onOpenNetwork,
}: {
  latestSample: AgentTelemetrySample | null;
  agentMode?: string | null;
  isReady: boolean;
  busy: boolean;
  onCollectSample: () => Promise<AgentTelemetrySample>;
  onSetTelemetryMode: (mode: "normal" | "realtime") => Promise<void>;
  /** Navega para Meu PC › Disco (link sob o indicador de disco). */
  onOpenDiskUsage?: () => void;
  /** Navega para Meu PC › Rede (link sob o indicador de latência). */
  onOpenNetwork?: () => void;
}) {
  const { t } = useI18n();
  const track = useTelemetry("telemetry");
  const live = useLiveTelemetry();
  const history = useTelemetryHistory();
  const [collected, setCollected] = useState<AgentTelemetrySample | null>(null);
  const [collecting, setCollecting] = useState(false);
  const [togglingMode, setTogglingMode] = useState(false);
  const [selectedKey, setSelectedKey] = useState<TelemetryMetricKey>("cpu");
  const [windowKey, setWindowKey] = useState<WindowKey>("1m");
  const [showWindow, setShowWindow] = useState(false);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const realtimeActive = agentMode === "realtime";

  // A amostra mais nova entre a prop, o store ao vivo e a última "Ler agora".
  const sample = useMemo(() => {
    const candidates = [latestSample, live, collected].filter((item): item is AgentTelemetrySample => Boolean(item));
    return candidates.reduce<AgentTelemetrySample | null>(
      (newest, item) => (!newest || item.event_timestamp > newest.event_timestamp ? item : newest),
      null,
    );
  }, [collected, latestSample, live]);

  useEffect(() => {
    recordTelemetrySample(latestSample);
  }, [latestSample]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const collect = useCallback(async () => {
    if (collecting) return;
    setCollecting(true);
    try {
      const next = await onCollectSample();
      recordTelemetrySample(next);
      setCollected(next);
      track("collect_sample_clicked");
    } catch (error) {
      track("collect_sample_failed");
      toast({ title: t("mypc.performance.actions.collectFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setCollecting(false);
    }
  }, [collecting, onCollectSample, t, track]);

  const toggleRealtime = useCallback(async () => {
    if (!isReady || busy || togglingMode) return;
    const nextMode = realtimeActive ? "normal" : "realtime";
    setTogglingMode(true);
    try {
      await onSetTelemetryMode(nextMode);
      track("local_realtime_toggled", { enabled: nextMode === "realtime" });
    } catch (error) {
      toast({ title: errorText(error), variant: "destructive" });
    } finally {
      setTogglingMode(false);
    }
  }, [busy, isReady, onSetTelemetryMode, realtimeActive, togglingMode, track]);

  const metric = METRICS.find((item) => item.key === selectedKey) ?? METRICS[0];
  const currentValues = sample ? readTelemetryValues(sample) : null;
  const selectedValue = currentValues?.[metric.key] ?? null;

  const windowDef = WINDOWS[windowKey];
  const windowStart = now - windowDef.seconds;
  const windowPoints = useMemo(
    () => history.points.filter((point) => point.t >= windowStart),
    [history.points, windowStart],
  );
  const values = windowPoints
    .map((point) => point.values[metric.key])
    .filter((value): value is number => value !== null);
  const stats = values.length
    ? {
        min: Math.min(...values),
        avg: values.reduce((sum, value) => sum + value, 0) / values.length,
        max: Math.max(...values),
      }
    : null;

  const recordedFor = history.startedAt !== null ? Math.max(0, now - history.startedAt) : 0;
  const windowLabel =
    recordedFor < windowDef.seconds
      ? t("mypc.performance.window.sinceStart", { duration: formatDuration(recordedFor) })
      : t(
          windowKey === "1m"
            ? "mypc.performance.window.lastMinute"
            : windowKey === "10m"
              ? "mypc.performance.window.lastTenMinutes"
              : "mypc.performance.window.lastHour",
        );

  const interval = readingInterval(history.points);
  const age = sample ? Math.max(0, Math.round(now - sample.event_timestamp)) : null;
  const notReadyReason = !isReady ? t("mypc.performance.actions.notReady") : null;

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 max-w-3xl">
          <SectionLabel className="flex items-center gap-2 text-fg-4">
            <span
              aria-hidden
              className={cn("h-1.5 w-1.5 rounded-full", sample ? "bg-ok" : "bg-fg-4", realtimeActive && "animate-pulse")}
            />
            {t("mypc.performance.eyebrow")}
          </SectionLabel>
          <p className="mt-1.5 text-sm text-fg-2">
            {interval !== null
              ? t("mypc.performance.liveEvery", { interval: formatDuration(interval) })
              : t("mypc.performance.liveWaiting")}
          </p>
        </div>
        <Segmented<WindowKey>
          variant="pill"
          aria-label={t("mypc.performance.window.label")}
          value={windowKey}
          onChange={setWindowKey}
          options={[
            { value: "1m", label: t("mypc.performance.window.oneMinute") },
            { value: "10m", label: t("mypc.performance.window.tenMinutes") },
            { value: "1h", label: t("mypc.performance.window.oneHour") },
          ]}
        />
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="text-xs text-fg-3">
          {age !== null && <span>{t("mypc.performance.lastReading", { age: formatDuration(age) })}</span>}
          {age !== null && <span aria-hidden> · </span>}
          <span>{t("mypc.performance.readingsInWindow", { count: windowPoints.length })}</span>
          {realtimeActive && (
            <>
              <span aria-hidden> · </span>
              <span className="text-ok">{t("mypc.performance.actions.realtimeActive")}</span>
            </>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            aria-pressed={realtimeActive}
            disabled={busy || togglingMode}
            disabledReason={notReadyReason}
            title={realtimeActive ? undefined : t("mypc.performance.actions.realtimeHint")}
            onClick={() => void toggleRealtime()}
          >
            <Gauge />
            {realtimeActive ? t("mypc.performance.actions.realtimeOff") : t("mypc.performance.actions.realtimeOn")}
          </Button>
          <Button variant="outline" size="sm" disabled={busy || collecting} onClick={() => void collect()}>
            <RefreshCw className={cn(collecting && "animate-spin")} />
            {collecting ? t("mypc.performance.actions.collecting") : t("mypc.performance.actions.collect")}
          </Button>
        </div>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_300px]">
        <div className="flex min-w-0 flex-col gap-3">
          {!sample && history.points.length === 0 ? (
            <EmptyState
              title={t("mypc.performance.noSample")}
              reason={t("mypc.performance.noSampleReason")}
              className="min-h-[380px] justify-center"
            />
          ) : (
            <Card padding="md" className="flex flex-col gap-4">
              <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
                <span className="text-sm text-fg-2">{t(`mypc.performance.metric.${metric.key}`)}</span>
                {selectedValue !== null ? (
                  <>
                    <span className="num text-[40px] font-semibold leading-none tracking-tight text-fg">
                      {formatValue(metric, selectedValue)}
                    </span>
                    <StatusBadge status={metric.status(selectedValue)} />
                  </>
                ) : (
                  <EmptyState
                    variant="inline"
                    title={sample ? t("mypc.performance.noReading") : t("mypc.performance.waiting")}
                    reason={sample ? t("mypc.performance.noReadingReason") : undefined}
                  />
                )}
                <span className="num text-xs text-fg-3">
                  {stats
                    ? t("mypc.performance.stats", {
                        min: formatValue(metric, stats.min),
                        avg: formatValue(metric, stats.avg),
                        max: formatValue(metric, stats.max),
                        window: windowLabel,
                      })
                    : t("mypc.performance.statsEmpty")}
                </span>
              </div>

              <MetricDetail
                metric={metric}
                sample={sample}
                onOpenDiskUsage={onOpenDiskUsage}
                onOpenNetwork={onOpenNetwork}
              />

              <LineChart
                metric={metric}
                points={windowPoints}
                windowKey={windowKey}
                windowStart={windowStart}
              />
            </Card>
          )}

          <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
            {METRICS.map((item) => (
              <Indicator
                key={item.key}
                metric={item}
                label={t(`mypc.performance.metric.${item.key}`)}
                value={currentValues?.[item.key] ?? null}
                hasSample={Boolean(sample)}
                selected={item.key === metric.key}
                onSelect={() => setSelectedKey(item.key)}
              />
            ))}
          </div>
        </div>

        <SystemPanel sample={sample} showWindow={showWindow} onToggleWindow={() => setShowWindow((v) => !v)} />
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: Status }) {
  const { t } = useI18n();
  return (
    <StatusPill tone={status === "good" ? "ok" : "warn"} className={cn(status === "critical" && "border-danger/50 text-danger")}>
      {t(`mypc.performance.status.${status}`)}
    </StatusPill>
  );
}

function Indicator({
  metric,
  label,
  value,
  hasSample,
  selected,
  onSelect,
}: {
  metric: MetricDef;
  label: string;
  value: number | null;
  hasSample: boolean;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        "flex min-h-[64px] flex-col justify-between gap-2 rounded-control border px-3.5 py-3 text-left transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
        selected ? "border-accent/70 bg-accent/5" : "border-line bg-surface hover:border-fg-4/60",
      )}
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="truncate text-[13px] text-fg-2">{label}</span>
        {value !== null && <span className="num text-[15px] font-semibold text-fg">{formatValue(metric, value)}</span>}
      </div>
      {value !== null ? (
        <Progress value={barPercent(metric, value)} label={`${label}: ${formatValue(metric, value)}`} tone={metric.tone} />
      ) : (
        <span className="text-xs text-fg-4">
          {hasSample ? t("mypc.performance.noReading") : t("mypc.performance.waiting")}
        </span>
      )}
    </button>
  );
}

function MetricDetail({
  metric,
  sample,
  onOpenDiskUsage,
  onOpenNetwork,
}: {
  metric: MetricDef;
  sample: AgentTelemetrySample | null;
  onOpenDiskUsage?: () => void;
  onOpenNetwork?: () => void;
}) {
  const { t } = useI18n();
  const text = sample ? metricDetail(metric.key, sample, t) : null;
  const link =
    metric.key === "disk" && onOpenDiskUsage ? (
      <Button variant="link" size="sm" onClick={onOpenDiskUsage}>
        {t("mypc.performance.openDisk")}
      </Button>
    ) : metric.key === "latency" && onOpenNetwork ? (
      <Button variant="link" size="sm" onClick={onOpenNetwork}>
        {t("mypc.performance.openNetwork")}
      </Button>
    ) : null;
  if (!text && !link) return null;
  return (
    <div className="-mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-fg-3">
      {text && <span className="truncate">{text}</span>}
      {link}
    </div>
  );
}

function metricDetail(
  key: TelemetryMetricKey,
  sample: AgentTelemetrySample,
  t: TFn,
): string | null {
  switch (key) {
    case "cpu": {
      const name = sample.cpu_name?.trim() ? shortCpuName(sample.cpu_name) : null;
      const ghz = sample.cpu_frequency_mhz ? formatGhz(sample.cpu_frequency_mhz) : null;
      return [name, ghz].filter(Boolean).join(" · ") || null;
    }
    case "gpu":
      return sample.gpu_name?.trim() || null;
    case "ram":
      return sample.ram_usage_mb > 0 && (sample.ram_total_mb ?? 0) > 0
        ? t("mypc.performance.detail.ofTotal", {
            used: formatMegabytes(sample.ram_usage_mb),
            total: formatMegabytes(sample.ram_total_mb),
          })
        : null;
    case "disk":
      return (sample.disk_used_gb ?? 0) > 0 && (sample.disk_total_gb ?? 0) > 0
        ? t("mypc.performance.detail.diskSpace", {
            used: formatGigabytes(sample.disk_used_gb),
            total: formatGigabytes(sample.disk_total_gb),
          })
        : null;
    case "vram":
      return (sample.vram_used_gb ?? 0) > 0 && (sample.vram_gb ?? 0) > 0
        ? t("mypc.performance.detail.ofTotal", {
            used: formatGigabytes(sample.vram_used_gb),
            total: formatGigabytes(sample.vram_gb),
          })
        : null;
    case "gpuTemp":
      return sample.gpu_temperature_available && sample.gpu_temperature_source
        ? t("mypc.performance.detail.tempSource", { source: temperatureSourceLabel(sample.gpu_temperature_source) })
        : null;
    case "cpuTemp":
      return sample.cpu_temperature_available && sample.cpu_temperature_source
        ? t("mypc.performance.detail.tempSource", { source: temperatureSourceLabel(sample.cpu_temperature_source) })
        : null;
    case "latency":
      return networkDetail(sample.network, t);
  }
}

/** Adaptador, perda e variação. Nunca o nome da rede Wi-Fi (SSID). */
function networkDetail(
  network: unknown,
  t: TFn,
): string | null {
  if (!network || typeof network !== "object") return null;
  const data = network as { adapter_name?: string | null; jitter_ms?: number | null; packet_loss_percent?: number | null };
  const parts = [
    data.adapter_name ? t("mypc.performance.detail.network", { adapter: data.adapter_name }) : null,
    typeof data.packet_loss_percent === "number"
      ? t("mypc.performance.detail.networkLoss", { value: formatPercent(data.packet_loss_percent) })
      : null,
    typeof data.jitter_ms === "number" && data.jitter_ms > 0
      ? t("mypc.performance.detail.networkJitter", { value: formatMs(data.jitter_ms) })
      : null,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : null;
}

const CHART_W = 1000;
const CHART_H = 300;

function LineChart({
  metric,
  points,
  windowKey,
  windowStart,
}: {
  metric: MetricDef;
  points: TelemetryPoint[];
  windowKey: WindowKey;
  windowStart: number;
}) {
  const { t } = useI18n();
  const { seconds, bucket } = WINDOWS[windowKey];
  const buckets = bucketize(points, metric.key, windowStart, bucket);
  const observedMax = buckets.reduce((max, item) => Math.max(max, item.value), 0);
  const scaleMax = Math.max(metric.scaleMax, observedMax * 1.1);
  const gapSeconds = Math.max(20, bucket * 3);

  const segments: Array<Array<{ x: number; y: number }>> = [];
  let previousT: number | null = null;
  for (const item of buckets) {
    const x = ((item.t - windowStart) / seconds) * CHART_W;
    const y = CHART_H - (Math.max(0, Math.min(scaleMax, item.value)) / scaleMax) * CHART_H;
    if (previousT === null || item.t - previousT > gapSeconds) segments.push([]);
    segments[segments.length - 1].push({ x: Math.max(0, Math.min(CHART_W, x)), y });
    previousT = item.t;
  }

  const color = cssVar(metric.tone);
  const axis =
    windowKey === "1m"
      ? ["oneMinuteStart", "oneMinuteMid"]
      : windowKey === "10m"
        ? ["tenMinutesStart", "tenMinutesMid"]
        : ["oneHourStart", "oneHourMid"];
  const unit = metric.kind === "percent" ? "%" : metric.kind === "ms" ? " ms" : " °C";
  const range = `0–${formatNumber(Math.round(scaleMax))}${unit}`;

  return (
    <div>
      <div className="relative h-[320px] w-full">
        <svg
          viewBox={`0 0 ${CHART_W} ${CHART_H}`}
          preserveAspectRatio="none"
          className="absolute inset-0 h-full w-full overflow-visible"
          role="img"
          aria-label={`${t(`mypc.performance.metric.${metric.key}`)} · ${t("mypc.performance.axis.scale", { range })}`}
        >
          {[0.25, 0.5, 0.75].map((line) => (
            <line
              key={line}
              x1={0}
              x2={CHART_W}
              y1={CHART_H * line}
              y2={CHART_H * line}
              stroke={cssVar("line")}
              strokeWidth={1}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {segments.map((segment, index) =>
            segment.length === 1 ? (
              <circle key={index} cx={segment[0].x} cy={segment[0].y} r={3} fill={color} vectorEffect="non-scaling-stroke" />
            ) : (
              <polyline
                key={index}
                points={segment.map((p) => `${p.x},${p.y}`).join(" ")}
                fill="none"
                stroke={color}
                strokeWidth={2}
                strokeLinecap="round"
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            ),
          )}
        </svg>
        <span className="pointer-events-none absolute right-0 top-0 text-[11px] text-fg-4">
          {t("mypc.performance.axis.scale", { range })}
        </span>
        {buckets.length < 2 && (
          <div className="absolute inset-0 grid place-items-center text-sm text-fg-3">
            {t("mypc.performance.chartEmpty")}
          </div>
        )}
      </div>
      <div className="mt-2 flex justify-between text-[11px] text-fg-4">
        <span>{t(`mypc.performance.axis.${axis[0]}`)}</span>
        <span>{t(`mypc.performance.axis.${axis[1]}`)}</span>
        <span>{t("mypc.performance.axis.now")}</span>
      </div>
    </div>
  );
}

/** Média por fatia de tempo (1 s, 3 s ou 15 s) para o gráfico de 1 hora não desenhar 3.600 pontos. */
function bucketize(points: TelemetryPoint[], key: TelemetryMetricKey, start: number, size: number) {
  const sums = new Map<number, { sum: number; count: number }>();
  for (const point of points) {
    const value = point.values[key];
    if (value === null) continue;
    const index = Math.floor((point.t - start) / size);
    const entry = sums.get(index) ?? { sum: 0, count: 0 };
    entry.sum += value;
    entry.count += 1;
    sums.set(index, entry);
  }
  return Array.from(sums.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([index, entry]) => ({ t: start + (index + 0.5) * size, value: entry.sum / entry.count }));
}

/** Intervalo real entre leituras (mediana das últimas 10), em segundos inteiros. */
function readingInterval(points: TelemetryPoint[]): number | null {
  const recent = points.slice(-11);
  if (recent.length < 3) return null;
  const gaps = recent
    .slice(1)
    .map((point, index) => point.t - recent[index].t)
    .filter((gap) => gap > 0)
    .sort((a, b) => a - b);
  if (!gaps.length) return null;
  return Math.max(1, Math.round(gaps[Math.floor(gaps.length / 2)]));
}

type SnapshotExtras = { active_profile?: string; advanced?: Record<string, unknown> | null };

function SystemPanel({
  sample,
  showWindow,
  onToggleWindow,
}: {
  sample: AgentTelemetrySample | null;
  showWindow: boolean;
  onToggleWindow: () => void;
}) {
  const { t } = useI18n();
  const extras = (sample ?? {}) as SnapshotExtras;
  // L1: mesma leitura do Dashboard ("O Chrome usa 9,6 GB").
  const topProcesses = useLiveTelemetry()?.top_processes ?? [];

  const contextKey = (() => {
    const active = extras.active_profile;
    if (active?.startsWith("focus_")) return active;
    return sample?.power_profile ?? null;
  })();
  const context =
    contextKey && contextKey in ptMypc.mypc.performance.contextValue
      ? t(`mypc.performance.contextValue.${contextKey}`)
      : null;

  const battery = extras.advanced ?? null;
  const batteryPercent = typeof battery?.battery_percent === "number" ? battery.battery_percent : null;
  const batteryStatus = typeof battery?.battery_status === "string" ? battery.battery_status.toLowerCase() : null;
  const onBattery = batteryStatus ? batteryStatus.includes("discharging") || batteryStatus.includes("descarregando") : false;
  const energy =
    batteryStatus === null
      ? null
      : batteryPercent !== null
        ? t(onBattery ? "mypc.performance.energyValue.batteryPercent" : "mypc.performance.energyValue.pluggedPercent", {
            percent: formatPercent(batteryPercent),
          })
        : t(onBattery ? "mypc.performance.energyValue.battery" : "mypc.performance.energyValue.plugged");

  const watts = typeof sample?.watts === "number" && sample.watts > 0 ? `${formatNumber(sample.watts)} W` : null;
  const power = watts
    ? t(sample?.is_estimated ? "mypc.performance.system.powerEstimated" : "mypc.performance.system.powerMeasured", {
        value: watts,
      })
    : null;

  const rows: Array<{ label: string; value: string }> = [];
  if (typeof sample?.active_processes === "number")
    rows.push({ label: t("mypc.performance.system.processes"), value: formatNumber(sample.active_processes) });
  if (sample?.system_uptime_seconds)
    rows.push({ label: t("mypc.performance.system.uptime"), value: formatDuration(sample.system_uptime_seconds) });
  if (context) rows.push({ label: t("mypc.performance.system.context"), value: context });
  if (energy) rows.push({ label: t("mypc.performance.system.energy"), value: energy });
  if (power) rows.push({ label: t("mypc.performance.system.power"), value: power });
  // Dados que o agente já coleta e a tela não mostrava: limitação térmica,
  // ventoinhas e energia da sessão (sempre com "estimado" quando for).
  if (sample?.throttling_suspected) {
    rows.push({ label: t("mypc.performance.system.thermal"), value: t("mypc.performance.system.throttling") });
  } else if (sample?.thermal_trend === "rising" || sample?.thermal_trend === "subindo") {
    rows.push({ label: t("mypc.performance.system.thermal"), value: t("mypc.performance.system.thermalRising") });
  }
  const fans = (sample?.fan_sensors ?? []).filter((fan) => typeof fan.value === "number" && fan.value > 0);
  if (fans.length > 0) {
    const fastest = fans.reduce((best, fan) => (fan.value > best.value ? fan : best), fans[0]);
    rows.push({
      label: t("mypc.performance.system.fans"),
      value: fans.length === 1 ? `${formatNumber(fastest.value)} ${fastest.unit || "rpm"}` : t("mypc.performance.system.fansMany", { count: fans.length, max: `${formatNumber(fastest.value)} ${fastest.unit || "rpm"}` }),
    });
  }
  if (typeof sample?.estimated_kwh === "number" && sample.estimated_kwh > 0) {
    rows.push({ label: t("mypc.performance.system.sessionEnergy"), value: t("mypc.performance.system.sessionEnergyValue", { value: formatNumber(sample.estimated_kwh) }) });
  }

  return (
    <Card label={t("mypc.performance.system.title")} className="flex flex-col">
      {sample ? (
        <div className="flex flex-col">
          {rows.map((row) => (
            <div key={row.label} className="flex items-baseline justify-between gap-3 border-t border-line-soft py-2.5 text-sm">
              <span className="text-fg-2">{row.label}</span>
              <span className="num text-right font-semibold text-fg">{row.value}</span>
            </div>
          ))}
          <div className="flex flex-col gap-1 border-t border-line-soft py-2.5 text-sm">
            <div className="flex items-baseline justify-between gap-3">
              <span className="text-fg-2">{t("mypc.performance.system.activeWindow")}</span>
              <Button variant="link" size="sm" aria-expanded={showWindow} onClick={onToggleWindow}>
                {showWindow ? t("mypc.performance.system.hide") : t("mypc.performance.system.show")}
              </Button>
            </div>
            {showWindow && (
              <span className="break-words text-fg">
                {sample.active_window?.trim() || t("mypc.performance.system.noWindow")}
              </span>
            )}
          </div>
        </div>
      ) : (
        <EmptyState variant="inline" title={t("mypc.performance.noSample")} />
      )}
      {topProcesses.length > 0 && (
        <div className="mt-4">
          <div className="label-mono">{t("mypc.performance.system.heaviest")}</div>
          <ul className="mt-2 flex flex-col gap-1.5 text-sm">
            {topProcesses.map((item, index) => (
              <li key={item.name} className="flex items-baseline gap-3">
                <span className="min-w-0 flex-1 truncate text-fg">{displayProcessName(item.name)}</span>
                <span className={index === 0 && item.memory_mb >= 4096 ? "num text-warn" : "num text-fg-2"}>
                  {formatMegabytes(item.memory_mb)}
                </span>
                <span className="num w-12 text-right text-fg-3">{formatPercent(item.cpu_percent)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <p className="mt-auto pt-6 text-xs leading-relaxed text-fg-4">{t("mypc.performance.system.privacy")}</p>
    </Card>
  );
}
