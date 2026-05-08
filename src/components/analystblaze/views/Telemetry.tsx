import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Cpu, MemoryStick, MonitorPlay, Thermometer, type LucideIcon } from "lucide-react";
import { TiltCard } from "../TiltCard";
import type { AgentTelemetrySample } from "@/services/tauri/agent";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";

type Metric = {
  key: string;
  labelKey: string;
  icon: LucideIcon;
  unit: string;
  min: number;
  max: number;
  hue: number;
  read: (sample: AgentTelemetrySample) => number;
};

const metrics: Metric[] = [
  { key: "cpu", labelKey: "telemetry.cpu", icon: Cpu, unit: "%", min: 0, max: 100, hue: 187, read: (s) => s.cpu_usage },
  { key: "gpu", labelKey: "telemetry.gpu", icon: MonitorPlay, unit: "%", min: 0, max: 100, hue: 265, read: (s) => s.gpu_usage },
  { key: "ram", labelKey: "telemetry.ram", icon: MemoryStick, unit: "MB", min: 0, max: 16_000, hue: 320, read: (s) => s.ram_usage_mb },
  { key: "temp", labelKey: "telemetry.gpuTemp", icon: Thermometer, unit: "C", min: 0, max: 95, hue: 150, read: (s) => s.gpu_temperature },
];

const HISTORY_LEN = 24;
const POLL_MS = 5_000;

const initialSample: AgentTelemetrySample = {
  event_timestamp: Math.floor(Date.now() / 1000),
  cpu_usage: 24,
  gpu_usage: 18,
  gpu_name: "GPU",
  vram_gb: 0,
  ram_usage_mb: 4200,
  gpu_temperature: 48,
  latency_ms: 14,
};

export function Telemetry({
  latestSample,
  onCollectSample,
}: {
  latestSample: AgentTelemetrySample | null;
  onCollectSample: () => Promise<AgentTelemetrySample>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("telemetry");
  const inFlight = useRef(false);
  const [sample, setSample] = useState<AgentTelemetrySample>(latestSample ?? initialSample);
  const [history, setHistory] = useState<Record<string, number[]>>(() =>
    Object.fromEntries(metrics.map((metric) => [metric.key, Array(HISTORY_LEN).fill(metric.read(latestSample ?? initialSample))])),
  );

  const appendSample = useCallback((nextSample: AgentTelemetrySample) => {
    setSample(nextSample);
    setHistory((current) => {
      const next = { ...current };
      for (const metric of metrics) {
        next[metric.key] = [...(current[metric.key] ?? []).slice(1), metric.read(nextSample)];
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
    } catch {
      track("collect_sample_failed");
    } finally {
      inFlight.current = false;
    }
  }, [appendSample, onCollectSample, track]);

  useEffect(() => {
    if (latestSample) appendSample(latestSample);
  }, [appendSample, latestSample]);

  useEffect(() => {
    void collect();
    const id = window.setInterval(() => {
      if (document.visibilityState === "visible") {
        void collect();
      }
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [collect]);

  const historySeconds = useMemo(() => Math.round((HISTORY_LEN * POLL_MS) / 1000), []);

  return (
    <div className="flex flex-col gap-8">
      <header className="flex items-end justify-between">
        <div>
          <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.3em] text-cyan-400/70">
            <span className="h-1.5 w-1.5 rounded-full bg-cyan-400 animate-pulse" />
            {t("telemetry.eyebrow")}
          </div>
          <h1 className="mt-2 text-[36px] font-semibold tracking-tight text-slate-50">
            {t("telemetry.title")}
          </h1>
          <p className="text-sm text-slate-400">{t("telemetry.subtitle", { seconds: historySeconds })}</p>
        </div>
        <div className="hidden items-center gap-2 md:flex">
          <button
            onClick={() => {
              track("collect_now_clicked");
              void collect();
            }}
            className="rounded-xl border border-cyan-400/40 bg-gradient-to-r from-cyan-500/20 to-violet-500/10 px-3 py-2 font-mono text-[10px] uppercase tracking-widest text-cyan-100 transition-all hover:border-cyan-300/60"
          >
            {t("telemetry.collectNow")}
          </button>
          <div className="flex items-center gap-2 rounded-xl border border-cyan-500/15 bg-slate-900/40 px-3 py-2 font-mono text-[10px] uppercase tracking-widest text-slate-400">
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
            {t("telemetry.activeCollection")}
          </div>
        </div>
      </header>

      <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
        {metrics.map((m) => {
          const v = m.read(sample);
          const pct = m.unit === "%" ? v : Math.min(100, (v / m.max) * 100);
          const Icon = m.icon;
          return (
            <TiltCard key={m.key} intensity={8} className="h-56">
              <div className="flex h-full items-center gap-6 p-6">
                <Gauge value={pct} hue={m.hue} loadLabel={t("telemetry.load")} />
                <div className="flex flex-1 flex-col justify-between self-stretch py-1">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <Icon className="h-4 w-4" style={{ color: `hsl(${m.hue} 100% 65%)` }} />
                      <span className="font-mono text-[11px] uppercase tracking-[0.25em] text-slate-400">
                        {t(m.labelKey)}
                      </span>
                    </div>
                    <span className="rounded-md border border-cyan-500/15 bg-slate-950/60 px-2 py-0.5 font-mono text-[9px] uppercase tracking-widest text-slate-500">
                      {t("telemetry.live")}
                    </span>
                  </div>
                  <div>
                    <div className="flex items-baseline gap-1.5">
                      <span className="text-4xl font-semibold tracking-tight text-slate-50 tabular-nums">
                        {v.toFixed(0)}
                      </span>
                      <span className="text-sm font-mono text-slate-500">{m.unit}</span>
                    </div>
                    <div className="mt-1 font-mono text-[10px] uppercase tracking-widest text-slate-500">
                      {t("telemetry.minMax", { max: m.max, min: m.min, unit: m.unit })}
                    </div>
                  </div>
                  <HistoryLine data={history[m.key]} hue={m.hue} />
                </div>
              </div>
            </TiltCard>
          );
        })}
      </div>
    </div>
  );
}

function Gauge({ value, hue, loadLabel }: { value: number; hue: number; loadLabel: string }) {
  const size = 120;
  const stroke = 9;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const offset = c - (Math.min(100, Math.max(0, value)) / 100) * c;
  return (
    <div className="relative shrink-0" style={{ width: size, height: size }}>
      <svg width={size} height={size} className="-rotate-90">
        <defs>
          <linearGradient id={`g-${hue}`} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor={`hsl(${hue} 100% 65%)`} />
            <stop offset="100%" stopColor={`hsl(${(hue + 60) % 360} 95% 65%)`} />
          </linearGradient>
        </defs>
        <circle cx={size / 2} cy={size / 2} r={r} stroke="hsl(222 40% 12%)" strokeWidth={stroke} fill="none" />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          stroke={`url(#g-${hue})`}
          strokeWidth={stroke}
          fill="none"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={offset}
          style={{ transition: "stroke-dashoffset 700ms ease-out", filter: `drop-shadow(0 0 6px hsl(${hue} 100% 60% / 0.6))` }}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-[10px] font-mono uppercase tracking-widest text-slate-500">{loadLabel}</span>
        <span className="text-2xl font-semibold tabular-nums text-slate-100">{value.toFixed(0)}<span className="text-xs text-slate-500">%</span></span>
      </div>
    </div>
  );
}

function HistoryLine({ data, hue }: { data: number[]; hue: number }) {
  const w = 200;
  const h = 28;
  const max = Math.max(...data, 1);
  const points = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (v / max) * h;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(" ");
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className="mt-2 h-7 w-full opacity-90">
      <polyline
        points={points}
        fill="none"
        stroke={`hsl(${hue} 100% 65%)`}
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ filter: `drop-shadow(0 0 3px hsl(${hue} 100% 60% / 0.6))` }}
      />
    </svg>
  );
}
