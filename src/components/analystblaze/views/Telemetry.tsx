import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity,
  BarChart3,
  Cpu,
  Gauge,
  HardDrive,
  MemoryStick,
  MonitorPlay,
  Radio,
  RefreshCw,
  Thermometer,
  Timer,
  Wifi,
  Zap,
  type LucideIcon,
} from "lucide-react";
import type { AgentTelemetrySample } from "@/services/tauri/agent";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";

type HistoryValue = number | null;

type Metric = {
  key: string;
  labelKey: string;
  icon: LucideIcon;
  unit: string;
  max: number;
  hue: number;
  read: (sample: AgentTelemetrySample) => number;
  available?: (sample: AgentTelemetrySample) => boolean;
  detail?: (sample: AgentTelemetrySample) => string;
  status?: (value: number | null) => "good" | "watch" | "critical";
};

const HISTORY_LEN = 90;

const metrics: Metric[] = [
  {
    key: "cpu",
    labelKey: "telemetry.cpu",
    icon: Cpu,
    unit: "%",
    max: 100,
    hue: 187,
    read: (s) => s.cpu_usage,
    detail: cpuTemperatureDetail,
    status: percentStatus,
  },
  {
    key: "gpu",
    labelKey: "telemetry.gpu",
    icon: MonitorPlay,
    unit: "%",
    max: 100,
    hue: 265,
    read: (s) => s.gpu_usage,
    available: (s) => Boolean(s.gpu_usage_available),
    detail: (s) => s.gpu_name || "GPU",
    status: percentStatus,
  },
  {
    key: "ram",
    labelKey: "telemetry.ram",
    icon: MemoryStick,
    unit: "%",
    max: 100,
    hue: 320,
    read: (s) => s.ram_usage_percent ?? 0,
    detail: (s) => `${formatMb(s.ram_usage_mb)} / ${formatMb(s.ram_total_mb ?? 0)}`,
    status: (value) => (value == null ? "good" : value >= 92 ? "critical" : value >= 82 ? "watch" : "good"),
  },
  {
    key: "disk",
    labelKey: "telemetry.disk",
    icon: HardDrive,
    unit: "%",
    max: 100,
    hue: 42,
    read: (s) => s.disk_usage_percent ?? 0,
    detail: (s) => `${formatGb(s.disk_used_gb ?? 0)} / ${formatGb(s.disk_total_gb ?? 0)}`,
    status: (value) => (value == null ? "good" : value >= 95 ? "critical" : value >= 88 ? "watch" : "good"),
  },
  {
    key: "latency",
    labelKey: "telemetry.latency",
    icon: Wifi,
    unit: "ms",
    max: 150,
    hue: 150,
    read: (s) => s.latency_ms ?? 0,
    available: (s) => Boolean(s.latency_ms && s.latency_ms > 0),
    detail: (s) => networkDetail(s.network),
    status: (value) => (value == null ? "good" : value >= 110 ? "critical" : value >= 70 ? "watch" : "good"),
  },
  {
    key: "cpu_temp",
    labelKey: "telemetry.cpuTemp",
    icon: Thermometer,
    unit: "C",
    max: 100,
    hue: 12,
    read: (s) => s.cpu_temperature ?? 0,
    available: (s) => Boolean(s.cpu_temperature_available),
    detail: cpuTemperatureDetail,
    status: tempStatus,
  },
  {
    key: "gpu_temp",
    labelKey: "telemetry.gpuTemp",
    icon: Thermometer,
    unit: "C",
    max: 95,
    hue: 350,
    read: (s) => s.gpu_temperature ?? 0,
    available: (s) => Boolean(s.gpu_temperature_available),
    status: tempStatus,
  },
  {
    key: "vram",
    labelKey: "telemetry.vram",
    icon: Activity,
    unit: "%",
    max: 100,
    hue: 220,
    read: (s) => s.vram_usage_percent ?? 0,
    available: (s) => s.vram_usage_percent != null,
    detail: (s) => `${formatGb(s.vram_used_gb ?? 0)} / ${formatGb(s.vram_gb ?? 0)}`,
    status: percentStatus,
  },
];

const defaultSelectedMetric = "cpu";

export function Telemetry({
  latestSample,
  agentMode,
  isReady,
  busy,
  onCollectSample,
  onSetTelemetryMode,
}: {
  latestSample: AgentTelemetrySample | null;
  agentMode?: string | null;
  isReady: boolean;
  busy: boolean;
  onCollectSample: () => Promise<AgentTelemetrySample>;
  onSetTelemetryMode: (mode: "normal" | "realtime") => Promise<void>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("telemetry");
  const inFlight = useRef(false);
  const [sample, setSample] = useState<AgentTelemetrySample | null>(latestSample);
  const [selectedKey, setSelectedKey] = useState(defaultSelectedMetric);
  const [history, setHistory] = useState<Record<string, HistoryValue[]>>(() =>
    Object.fromEntries(metrics.map((metric) => [metric.key, []])),
  );
  const [timestamps, setTimestamps] = useState<number[]>([]);
  const realtimeActive = agentMode === "realtime";
  const selectedMetric = metrics.find((metric) => metric.key === selectedKey) ?? metrics[0];
  const selectedData = history[selectedMetric.key] ?? [];

  const appendSample = useCallback((nextSample: AgentTelemetrySample) => {
    setSample(nextSample);
    setTimestamps((current) => [...current.slice(-(HISTORY_LEN - 1)), nextSample.event_timestamp]);
    setHistory((current) => {
      const next = { ...current };
      for (const metric of metrics) {
        const available = metric.available ? metric.available(nextSample) : true;
        next[metric.key] = [
          ...(current[metric.key] ?? []).slice(-(HISTORY_LEN - 1)),
          available ? metric.read(nextSample) : null,
        ];
      }
      return next;
    });
  }, []);

  const collect = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const nextSample = await onCollectSample();
      appendSample(nextSample);
      track("collect_sample_clicked");
    } catch {
      track("collect_sample_failed");
    } finally {
      inFlight.current = false;
    }
  }, [appendSample, onCollectSample, track]);

  const toggleRealtime = useCallback(async () => {
    if (!isReady || busy) return;
    const nextMode = realtimeActive ? "normal" : "realtime";
    await onSetTelemetryMode(nextMode);
    track("local_realtime_toggled", { enabled: nextMode === "realtime" });
  }, [busy, isReady, onSetTelemetryMode, realtimeActive, track]);

  useEffect(() => {
    if (latestSample) appendSample(latestSample);
  }, [appendSample, latestSample]);

  const selectedValue = readMetricValue(sample, selectedMetric);
  const selectedStatus = selectedMetric.status?.(selectedValue) ?? "good";
  const stats = metricStats(selectedData);
  const age = sample ? Math.max(0, Math.round(Date.now() / 1000 - sample.event_timestamp)) : null;
  const rate = sampleRate(timestamps);
  const systemSignals = systemSignalsFor(sample, t);

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
            <span className={`h-1.5 w-1.5 rounded-full ${realtimeActive ? "animate-pulse bg-emerald-400" : "bg-cyan-400"}`} />
            {t("telemetry.eyebrow")}
          </div>
          <h1 className="mt-2 text-[34px] font-semibold tracking-tight text-slate-50">
            {t("telemetry.title")}
          </h1>
          <p className="mt-1 max-w-2xl text-sm leading-relaxed text-slate-400">
            {t("telemetry.subtitle", { seconds: HISTORY_LEN })}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            role="switch"
            aria-checked={realtimeActive}
            disabled={busy || !isReady}
            onClick={() => void toggleRealtime()}
            className={`inline-flex min-h-11 items-center gap-3 rounded-xl border px-4 py-2 text-sm font-semibold transition-all disabled:opacity-50 ${
              realtimeActive
                ? "border-emerald-300/50 bg-emerald-400/15 text-emerald-100 shadow-[0_0_28px_-10px_hsl(150_90%_55%/0.8)]"
                : "border-cyan-400/40 bg-cyan-400/10 text-cyan-100 hover:border-cyan-300/60"
            }`}
          >
            {realtimeActive ? <Radio className="h-4 w-4" /> : <Zap className="h-4 w-4" />}
            {realtimeActive ? t("telemetry.disableRealtime") : t("telemetry.enableRealtime")}
          </button>
          <button
            disabled={busy}
            onClick={() => void collect()}
            className="inline-flex min-h-11 items-center gap-2 rounded-xl border border-cyan-400/30 bg-slate-950/55 px-3 py-2 font-mono text-[10px] uppercase tracking-widest text-cyan-100 transition-all hover:border-cyan-300/60 disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${inFlight.current ? "animate-spin" : ""}`} />
            {t("telemetry.collectNow")}
          </button>
        </div>
      </header>

      <section className="glass-panel cyber-glow overflow-hidden">
        <div className="grid min-h-[420px] xl:grid-cols-[1fr_320px]">
          <div className="flex min-w-0 flex-col gap-4 p-5">
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div className="flex min-w-0 items-center gap-3">
                <MetricIcon metric={selectedMetric} />
                <div className="min-w-0">
                  <div className="font-mono text-[10px] uppercase tracking-[0.26em] text-slate-500">
                    {t(selectedMetric.labelKey)}
                  </div>
                  <div className="mt-1 flex flex-wrap items-baseline gap-2">
                    <span className="text-5xl font-semibold tracking-tight text-slate-50 tabular-nums">
                      {selectedValue == null ? "--" : formatMetricValue(selectedValue, selectedMetric.unit)}
                    </span>
                    <span className="font-mono text-sm text-slate-500">{selectedMetric.unit}</span>
                    <StatusBadge status={selectedStatus} />
                  </div>
                  {sample && selectedMetric.detail && (
                    <div className="mt-1 max-w-[520px] truncate text-xs text-slate-500">
                      {selectedMetric.detail(sample)}
                    </div>
                  )}
                </div>
              </div>

              <div className="grid grid-cols-3 gap-2 lg:w-[280px]">
                <MiniStat label="min" value={stats.min == null ? "--" : formatMetricValue(stats.min, selectedMetric.unit)} />
                <MiniStat label="avg" value={stats.avg == null ? "--" : formatMetricValue(stats.avg, selectedMetric.unit)} />
                <MiniStat label="max" value={stats.max == null ? "--" : formatMetricValue(stats.max, selectedMetric.unit)} />
              </div>
            </div>

            <MainChart metric={selectedMetric} data={selectedData} />

            <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
              {metrics.slice(0, 8).map((metric) => (
                <MetricSelector
                  key={metric.key}
                  metric={metric}
                  sample={sample}
                  selected={metric.key === selectedMetric.key}
                  data={history[metric.key] ?? []}
                  onSelect={() => setSelectedKey(metric.key)}
                  label={t(metric.labelKey)}
                />
              ))}
            </div>
          </div>

          <aside className="border-t border-cyan-500/10 bg-slate-950/45 p-5 xl:border-l xl:border-t-0">
            <div className="grid grid-cols-2 gap-2">
              <FrameTile label={t("telemetry.mode")} value={realtimeActive ? t("telemetry.realtimeMode") : t("telemetry.normalMode")} active={realtimeActive} />
              <FrameTile label={t("telemetry.rate")} value={rate ? `${rate.toFixed(1)}/s` : "--"} />
              <FrameTile label={t("telemetry.points")} value={timestamps.length.toString()} />
              <FrameTile label={t("telemetry.lastFrame")} value={age == null ? "--" : `${age}s`} />
            </div>

            <div className="mt-4 rounded-xl border border-emerald-400/15 bg-emerald-400/10 px-3 py-2">
              <div className="font-mono text-[10px] uppercase tracking-widest text-emerald-200">
                {t("telemetry.localOnly")}
              </div>
              <div className="mt-1 text-xs leading-relaxed text-emerald-100/75">
                {t("telemetry.backendStorageOff")}
              </div>
            </div>

            <div className="mt-5 flex flex-col gap-2">
              <div className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.25em] text-slate-500">
                <BarChart3 className="h-3.5 w-3.5 text-cyan-300" />
                {t("telemetry.systemSignals")}
              </div>
              {systemSignals.map((signal) => (
                <SignalRow key={signal.label} {...signal} />
              ))}
            </div>
          </aside>
        </div>
      </section>

      {!sample && (
        <div className="rounded-xl border border-cyan-500/10 bg-slate-950/45 p-5 text-sm text-slate-500">
          {t("dashboard.noTelemetryDesc")}
        </div>
      )}
    </div>
  );
}

function MetricIcon({ metric }: { metric: Metric }) {
  const Icon = metric.icon;
  return (
    <div
      className="grid h-12 w-12 shrink-0 place-items-center rounded-xl border bg-slate-950/65"
      style={{
        borderColor: `hsl(${metric.hue} 100% 65% / 0.26)`,
        boxShadow: `0 0 24px -14px hsl(${metric.hue} 100% 60%)`,
      }}
    >
      <Icon className="h-5 w-5" style={{ color: `hsl(${metric.hue} 100% 65%)` }} />
    </div>
  );
}

function MainChart({ metric, data }: { metric: Metric; data: HistoryValue[] }) {
  const width = 760;
  const height = 260;
  const points = chartPoints(data, metric.max, width, height);
  const area = points ? `0,${height} ${points} ${width},${height}` : "";

  return (
    <div className="relative min-h-[300px] overflow-hidden rounded-xl border border-cyan-500/10 bg-slate-950/45">
      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="absolute inset-0 h-full w-full">
        <defs>
          <linearGradient id={`telemetry-main-${metric.key}`} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0%" stopColor={`hsl(${metric.hue} 100% 60%)`} stopOpacity="0.34" />
            <stop offset="100%" stopColor={`hsl(${metric.hue} 100% 60%)`} stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {[0.2, 0.4, 0.6, 0.8].map((line) => (
          <line key={line} x1="0" x2={width} y1={height * line} y2={height * line} stroke="hsl(197 40% 24% / 0.32)" strokeDasharray="5 12" />
        ))}
        {area && <polygon points={area} fill={`url(#telemetry-main-${metric.key})`} />}
        {points ? (
          <polyline
            points={points}
            fill="none"
            stroke={`hsl(${metric.hue} 100% 65%)`}
            strokeWidth="2.4"
            strokeLinecap="round"
            strokeLinejoin="round"
            vectorEffect="non-scaling-stroke"
          />
        ) : (
          <line x1="24" x2={width - 24} y1={height / 2} y2={height / 2} stroke="hsl(215 20% 35%)" strokeDasharray="3 7" />
        )}
      </svg>
      <div className="pointer-events-none absolute left-4 top-4 font-mono text-[10px] uppercase tracking-widest text-slate-600">
        0 - {metric.max}{metric.unit}
      </div>
    </div>
  );
}

function MetricSelector({
  metric,
  sample,
  selected,
  data,
  onSelect,
  label,
}: {
  metric: Metric;
  sample: AgentTelemetrySample | null;
  selected: boolean;
  data: HistoryValue[];
  onSelect: () => void;
  label: string;
}) {
  const Icon = metric.icon;
  const value = readMetricValue(sample, metric);
  const pct = value == null ? 0 : metric.unit === "%" ? value : Math.min(100, (value / metric.max) * 100);

  return (
    <button
      onClick={onSelect}
      className={`min-h-[86px] rounded-xl border p-3 text-left transition-all ${
        selected
          ? "border-cyan-300/45 bg-cyan-400/10 shadow-[0_0_25px_-16px_hsl(187_100%_60%/0.8)]"
          : "border-cyan-500/10 bg-slate-950/35 hover:border-cyan-400/30 hover:bg-slate-950/55"
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Icon className="h-3.5 w-3.5 shrink-0" style={{ color: `hsl(${metric.hue} 100% 65%)` }} />
          <span className="truncate font-mono text-[10px] uppercase tracking-widest text-slate-500">{label}</span>
        </div>
        <span className="text-sm font-semibold text-slate-100 tabular-nums">
          {value == null ? "--" : formatMetricValue(value, metric.unit)}
        </span>
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-slate-800">
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{ width: `${Math.max(0, Math.min(100, pct))}%`, backgroundColor: `hsl(${metric.hue} 100% 60%)` }}
        />
      </div>
      <TinyLine data={data} max={metric.max} hue={metric.hue} />
    </button>
  );
}

function TinyLine({ data, max, hue }: { data: HistoryValue[]; max: number; hue: number }) {
  const points = chartPoints(data, max, 100, 22);
  return (
    <svg viewBox="0 0 100 22" preserveAspectRatio="none" className="mt-2 h-[22px] w-full opacity-80">
      {points ? (
        <polyline points={points} fill="none" stroke={`hsl(${hue} 100% 65%)`} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      ) : (
        <line x1="0" x2="100" y1="16" y2="16" stroke="hsl(215 20% 35%)" strokeDasharray="3 6" />
      )}
    </svg>
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

function FrameTile({ label, value, active = false }: { label: string; value: string; active?: boolean }) {
  return (
    <div className={`rounded-xl border px-3 py-3 ${active ? "border-emerald-400/20 bg-emerald-400/10" : "border-cyan-500/10 bg-slate-950/45"}`}>
      <div className="font-mono text-[9px] uppercase tracking-widest text-slate-500">{label}</div>
      <div className={`mt-1 text-base font-semibold tabular-nums ${active ? "text-emerald-100" : "text-slate-100"}`}>{value}</div>
    </div>
  );
}

function SignalRow({
  icon: Icon,
  label,
  value,
  tone,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  tone: "good" | "watch" | "critical" | "neutral";
}) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-cyan-500/10 bg-slate-950/35 px-3 py-2.5">
      <Icon className={`h-4 w-4 shrink-0 ${signalTone(tone)}`} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium text-slate-200">{label}</div>
        <div className="truncate text-xs text-slate-500">{value}</div>
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: "good" | "watch" | "critical" }) {
  const label = status === "critical" ? "critico" : status === "watch" ? "atencao" : "estavel";
  return (
    <span className={`rounded-md border px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest ${badgeTone(status)}`}>
      {label}
    </span>
  );
}

function readMetricValue(sample: AgentTelemetrySample | null, metric: Metric) {
  if (!sample) return null;
  if (metric.available && !metric.available(sample)) return null;
  const value = metric.read(sample);
  return Number.isFinite(value) ? value : null;
}

function chartPoints(data: HistoryValue[], max: number, width: number, height: number) {
  const values = data.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (values.length < 2) return "";
  return values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * width;
      const y = height - (Math.max(0, Math.min(max, value)) / max) * height;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

function metricStats(values: HistoryValue[]) {
  const numeric = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (!numeric.length) return { min: null, avg: null, max: null };
  return {
    min: Math.min(...numeric),
    avg: numeric.reduce((sum, value) => sum + value, 0) / numeric.length,
    max: Math.max(...numeric),
  };
}

function sampleRate(timestamps: number[]) {
  const unique = Array.from(new Set(timestamps)).sort((a, b) => a - b);
  if (unique.length < 2) return null;
  const duration = unique[unique.length - 1] - unique[0];
  if (duration <= 0) return null;
  return (unique.length - 1) / duration;
}

function systemSignalsFor(sample: AgentTelemetrySample | null, t: (key: string) => string) {
  return [
    {
      icon: Thermometer,
      label: t("telemetry.cpuTemp"),
      value: cpuTemperatureMethodSummary(sample),
      tone: tempStatus(sample?.cpu_temperature_available ? sample?.cpu_temperature ?? null : null),
    },
    {
      icon: Activity,
      label: t("telemetry.processes"),
      value: sample?.active_processes?.toString() ?? "--",
      tone: (sample?.active_processes ?? 0) > 260 ? "watch" : "neutral",
    },
    {
      icon: Timer,
      label: t("telemetry.uptime"),
      value: formatDuration(sample?.system_uptime_seconds ?? 0),
      tone: "neutral",
    },
    {
      icon: Gauge,
      label: t("telemetry.activeWindow"),
      value: sample?.active_window || "--",
      tone: "neutral",
    },
    {
      icon: Wifi,
      label: t("telemetry.latency"),
      value: sample?.latency_ms ? `${Math.round(sample.latency_ms)} ms` : "--",
      tone: sample?.latency_ms && sample.latency_ms > 90 ? "watch" : "good",
    },
  ] as Array<{ icon: LucideIcon; label: string; value: string; tone: "good" | "watch" | "critical" | "neutral" }>;
}

function percentStatus(value: number | null) {
  if (value == null) return "good";
  if (value >= 90) return "critical";
  if (value >= 75) return "watch";
  return "good";
}

function tempStatus(value: number | null) {
  if (value == null) return "good";
  if (value >= 88) return "critical";
  if (value >= 78) return "watch";
  return "good";
}

function signalTone(tone: "good" | "watch" | "critical" | "neutral") {
  if (tone === "critical") return "text-rose-300";
  if (tone === "watch") return "text-amber-300";
  if (tone === "good") return "text-emerald-300";
  return "text-cyan-300";
}

function badgeTone(status: "good" | "watch" | "critical") {
  if (status === "critical") return "border-rose-400/30 bg-rose-400/10 text-rose-200";
  if (status === "watch") return "border-amber-400/30 bg-amber-400/10 text-amber-200";
  return "border-emerald-400/30 bg-emerald-400/10 text-emerald-200";
}

function formatMetricValue(value: number, unit: string) {
  if (!Number.isFinite(value)) return "--";
  if (unit === "ms") return value < 10 ? value.toFixed(1) : Math.round(value).toString();
  return Math.round(value).toString();
}

function formatMb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "--";
  if (value >= 1024) return `${(value / 1024).toFixed(1)} GB`;
  return `${Math.round(value)} MB`;
}

function formatGb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "--";
  return `${value.toFixed(1)} GB`;
}

function formatTemp(value?: number | null, available?: boolean) {
  if (!available || value == null || !Number.isFinite(value) || value <= 0) return "--";
  return `${Math.round(value)} C`;
}

function cpuTemperatureDetail(sample: AgentTelemetrySample) {
  const temp = formatTemp(sample.cpu_temperature, sample.cpu_temperature_available);
  if (temp === "--") return "sensor indisponivel";
  return `${temp} - ${temperatureSourceLabel(sample.cpu_temperature_source)}`;
}

function cpuTemperatureMethodSummary(sample: AgentTelemetrySample | null) {
  if (!sample?.cpu_temperature_methods?.length) {
    return sample?.cpu_temperature_available
      ? `${formatTemp(sample.cpu_temperature, true)} - ${temperatureSourceLabel(sample.cpu_temperature_source)}`
      : "--";
  }

  const available = sample.cpu_temperature_methods
    .filter((method) => method.available && method.value_c != null && Number.isFinite(method.value_c))
    .slice(0, 2);

  if (!available.length) {
    return sample.cpu_temperature_methods[0]?.label || "--";
  }

  return available
    .map((method) => `${temperatureSourceLabel(method.source)} ${formatTemp(method.value_c, true)}`)
    .join(" / ");
}

function temperatureSourceLabel(source?: string | null) {
  if (source === "sysinfo_cpu_sensor") return "sensor do sistema";
  if (source === "sysinfo_component_max") return "componente mais quente";
  if (source === "libre_hardware_monitor") return "LibreHardwareMonitor";
  if (source === "open_hardware_monitor") return "OpenHardwareMonitor";
  if (source === "acpi_thermal_zone") return "ACPI thermal zone";
  if (source === "external_wmi") return "WMI";
  return "fonte local";
}

function formatDuration(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "--";
  if (value < 60) return `${Math.round(value)}s`;
  const minutes = Math.floor(value / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function networkDetail(network: unknown) {
  if (!network || typeof network !== "object") return "rede local";
  const data = network as {
    adapter_name?: string | null;
    wifi_ssid?: string | null;
    jitter_ms?: number | null;
    packet_loss_percent?: number | null;
  };
  const name = data.wifi_ssid || data.adapter_name || "adaptador ativo";
  const loss = data.packet_loss_percent != null ? `${Math.round(data.packet_loss_percent)}% perda` : null;
  const jitter = data.jitter_ms != null ? `${Math.round(data.jitter_ms)} ms jitter` : null;
  return [name, loss, jitter].filter(Boolean).join(" - ");
}
