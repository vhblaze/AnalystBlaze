import { useId } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useI18n } from "@/i18n";
import { colors } from "@/design/tokens";

/**
 * Anel de Saúde do Dashboard: o número sem "%", "como é calculada" (os
 * motivos que o agente deu, por regras fixas) e "medir de novo". Saúde é a
 * leitura de agora; não é a Medição de desempenho (doc 14 Q3).
 */
export function HealthRing({
  score,
  reasons,
  onMeasure,
  measuring,
}: {
  score: number | null;
  reasons: string[];
  onMeasure: () => void;
  measuring: boolean;
}) {
  const { t } = useI18n();
  const gradientId = useId();
  const size = 108;
  const stroke = 9;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const value = score === null ? 0 : Math.max(0, Math.min(100, score));

  return (
    <div className="flex w-[250px] shrink-0 flex-col items-center gap-3">
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
        <Tooltip>
          <TooltipTrigger asChild>
            <button type="button" className="text-fg-2 underline underline-offset-2 hover:text-fg">
              {t("dashboard.d3.howCalculated")}
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="max-w-xs border-line bg-surface p-3 text-fg">
            <p className="text-xs text-fg-3">{t("dashboard.d3.howCalculatedIntro")}</p>
            {reasons.length > 0 ? (
              <ul className="mt-2 list-disc space-y-1 pl-4 text-xs text-fg-2">
                {reasons.map((reason) => (
                  <li key={reason}>{reason}</li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 text-xs text-fg-2">{t("dashboard.d3.howCalculatedNoReasons")}</p>
            )}
          </TooltipContent>
        </Tooltip>
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
    </div>
  );
}
