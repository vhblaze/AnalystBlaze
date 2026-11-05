import { useEffect, useId, useRef, useState } from "react";
import { useI18n } from "@/i18n";
import { colors } from "@/design/tokens";
import { formatNumber } from "@/lib/format";
import type { HealthDeduction } from "@/services/tauri/agent";

/**
 * Anel de Saúde do Dashboard: o número sem "%", "como é calculada" (os
 * motivos que o agente deu, por regras fixas) e "medir de novo". Saúde é a
 * leitura de agora; não é a Medição de desempenho (doc 14 Q3).
 */
/** Fatores medidos em %; o resto é contagem ou °C. */
const PERCENT_FACTORS = new Set(["cpu_critical", "cpu_sustained_load", "ram_pressure_critical", "ram_pressure", "disk_space_low", "network_packet_loss"]);
const CELSIUS_FACTORS = new Set(["gpu_thermal_critical", "gpu_thermal_watch", "cpu_thermal_critical", "cpu_thermal_watch"]);
const MS_FACTORS = new Set(["network_jitter"]);

function unitFor(factor: string): string {
  if (PERCENT_FACTORS.has(factor)) return "%";
  if (CELSIUS_FACTORS.has(factor)) return " °C";
  if (MS_FACTORS.has(factor)) return " ms";
  return "";
}

export function HealthRing({
  score,
  reasons,
  breakdown,
  onMeasure,
  measuring,
}: {
  score: number | null;
  reasons: string[];
  /** L3: a conta ponto a ponto. Vazio em agentes antigos: aí mostra só os motivos. */
  breakdown: HealthDeduction[];
  onMeasure: () => void;
  measuring: boolean;
}) {
  const { t } = useI18n();
  const gradientId = useId();
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!boxRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);
  const size = 108;
  const stroke = 9;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const value = score === null ? 0 : Math.max(0, Math.min(100, score));

  return (
    <div ref={boxRef} className="relative flex w-[250px] shrink-0 flex-col items-center gap-3">
      <div className="relative" style={{ width: size, height: size }}>
        <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="-rotate-90" aria-hidden>
          <defs>
            <linearGradient id={gradientId} x1="0" y1="1" x2="1" y2="0">
              <stop offset="0" stopColor={colors.accent} />
              <stop offset="1" stopColor={colors.brandViolet} />
            </linearGradient>
          </defs>
          <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke={colors.line} strokeWidth={stroke} />
          {score !== null && (
            <circle
              cx={size / 2}
              cy={size / 2}
              r={radius}
              fill="none"
              stroke={`url(#${gradientId})`}
              strokeWidth={stroke}
              strokeLinecap="round"
              strokeDasharray={circumference}
              strokeDashoffset={circumference * (1 - value / 100)}
              className="transition-[stroke-dashoffset] duration-base ease-out"
            />
          )}
        </svg>
        <div
          className="num absolute inset-0 grid place-items-center text-[30px] font-semibold text-fg"
          aria-label={score === null ? t("dashboard.d3.healthNoReading") : t("dashboard.d3.healthAria", { score: Math.round(value) })}
        >
          {score === null ? <span className="text-sm font-normal text-fg-3">{t("dashboard.d3.healthNoReadingShort")}</span> : Math.round(value)}
        </div>
      </div>
      <div className="flex items-center justify-center gap-x-1.5 whitespace-nowrap text-xs text-fg-3">
        <span>{t("dashboard.d3.health")}</span>
        <span aria-hidden>·</span>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={() => setOpen((value) => !value)}
          className="text-fg-2 underline underline-offset-2 hover:text-fg"
        >
          {t("dashboard.d3.howCalculated")}
        </button>
        <span aria-hidden>·</span>
        <button
          type="button"
          disabled={measuring}
          onClick={onMeasure}
          className="text-fg-2 underline underline-offset-2 hover:text-fg disabled:no-underline disabled:opacity-60"
        >
          {measuring ? t("dashboard.d3.measuring") : t("dashboard.d3.measureAgain")}
        </button>
      </div>
      {open && (
        <div
          id={panelId}
          role="dialog"
          aria-label={t("dashboard.d3.howCalculated")}
          className="absolute left-0 top-full z-30 mt-2 w-[320px] rounded-card border border-line bg-surface p-4 text-left shadow-xl shadow-black/40"
        >
            <p className="text-xs text-fg-3">{t("dashboard.d3.howCalculatedIntro")}</p>
            {breakdown.length > 0 ? (
              <ul className="mt-2 space-y-1 text-xs text-fg-2">
                {breakdown.map((item) => {
                  const unit = unitFor(item.factor);
                  const measured =
                    typeof item.measured === "number"
                      ? t("dashboard.d3.factorMeasured", {
                          value: `${formatNumber(Math.round(item.measured))}${unit}`,
                          limit: typeof item.threshold === "number" ? `${formatNumber(item.threshold)}${unit}` : "-",
                        })
                      : null;
                  return (
                    <li key={item.factor} className="flex items-baseline justify-between gap-3">
                      <span>
                        {t(`dashboard.d3.factors.${item.factor}`)}
                        {measured && <span className="text-fg-3"> · {measured}</span>}
                      </span>
                      <span className="num shrink-0 font-semibold text-warn">−{item.points}</span>
                    </li>
                  );
                })}
                <li className="flex justify-between gap-3 border-t border-line pt-1 text-fg">
                  <span>{t("dashboard.d3.factorTotal")}</span>
                  <span className="num font-semibold">{score ?? 0}</span>
                </li>
              </ul>
            ) : reasons.filter((reason) => reason !== "stable").length > 0 ? (
              <ul className="mt-2 list-disc space-y-1 pl-4 text-xs text-fg-2">
                {reasons
                  .filter((reason) => reason !== "stable")
                  .map((reason) => (
                    <li key={reason}>{t(`dashboard.d3.factors.${reason}`)}</li>
                  ))}
              </ul>
            ) : (
              <p className="mt-2 text-xs text-fg-2">{t("dashboard.d3.howCalculatedNoReasons")}</p>
            )}
        </div>
      )}
    </div>
  );
}
