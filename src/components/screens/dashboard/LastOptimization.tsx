import { ArrowRight, Gauge } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { displayGameName, formatDateTime, formatNumber, formatSignedFps, formatSignedPercent } from "@/lib/format";
import { usePerformanceScan } from "@/features/performance/usePerformanceScan";
import { frameMeasurements, latestFrameMeasurement, skippedReasonKey } from "@/features/gamemode/useGameMode";
import type { FrameMeasurementEntry } from "@/services/tauri/agent";

/**
 * "Última otimização". Com uma medição de FPS do Modo Jogo automático
 * (lacuna L4), mostra o 1% low antes e depois no mesmo jogo, a diferença
 * com sinal e quanto o jogo varia sozinho. Sem ela, mostra a Medição de
 * desempenho, com esse nome (doc 14 Q3).
 */
export function LastOptimization({ disabledReason }: { disabledReason: string | null }) {
  const { t } = useI18n();
  const scan = usePerformanceScan({ auto: false });
  const measurements = frameMeasurements.use();
  const { valid, skippedAfter } = latestFrameMeasurement(measurements.data);

  const skippedNote = skippedAfter ? (
    <p className="text-xs text-warn">
      {t("dashboard.d3.fps.skipped", {
        when: formatDateTime(skippedAfter.measuredAt),
        reason: t(`dashboard.d3.fps.reason.${skippedReasonKey(skippedAfter.measurement.skipped)}`),
      })}
    </p>
  ) : null;

  if (valid) return <FpsBeforeAfter entry={valid} note={skippedNote} />;

  const report = scan.report;
  const measure = () => void scan.refresh("quick").catch(() => undefined);
  const hasBeforeAfter = report && report.previousScore != null;
  const gain = report ? (report.measuredGainPercent ?? report.scoreDeltaPercent ?? null) : null;

  return (
    <Card
      label={t("dashboard.d3.lastOptimization")}
      aside={report ? formatDateTime(report.generatedAt) : null}
      className="flex min-h-[248px] flex-col"
    >
      {!report ? (
        <div className="flex flex-1 flex-col">
          <EmptyState
            className="flex-1 border-none p-0"
            icon={<Gauge />}
            title={t("dashboard.d3.noBeforeAfterTitle")}
            reason={scan.error ?? t("dashboard.d3.noBeforeAfterReason")}
            action={
              <Button size="sm" variant="outline" onClick={measure} disabled={scan.loading} disabledReason={disabledReason}>
                {scan.loading ? t("dashboard.d3.measuring") : t("dashboard.d3.measureNow")}
              </Button>
            }
          />
          {skippedNote && <div className="pt-3">{skippedNote}</div>}
        </div>
      ) : (
        <div className="flex flex-1 flex-col">
          <div className="num flex items-baseline gap-3">
            {hasBeforeAfter && (
              <>
                <span className="text-[34px] font-semibold text-fg-3">{Math.round(report.previousScore as number)}</span>
                <ArrowRight className="h-5 w-5 self-center text-fg-3" aria-hidden />
              </>
            )}
            <span className="text-[34px] font-semibold text-ok">{Math.round(report.overallScore)}</span>
            <span className="text-sm text-fg-3">/100</span>
            {hasBeforeAfter && gain !== null && (
              <span className={gain >= 0 ? "text-sm font-semibold text-ok" : "text-sm font-semibold text-warn"}>
                {formatSignedPercent(gain)}
              </span>
            )}
          </div>
          <p className="mt-1 text-[13px] text-fg-3">
            {hasBeforeAfter ? t("dashboard.d3.beforeAfterCaption") : t("dashboard.d3.singleMeasureCaption")}
          </p>
          <div className="mt-auto flex flex-col gap-1 pt-4">
            {skippedNote}
            <p className="text-xs text-fg-3">{t("dashboard.d3.fpsPending")}</p>
          </div>
        </div>
      )}
    </Card>
  );
}

function fpsNumber(value: number | null | undefined): string {
  return typeof value === "number" && value > 0 ? String(Math.round(value)) : "0";
}

function FpsBeforeAfter({ entry, note }: { entry: FrameMeasurementEntry; note: React.ReactNode }) {
  const { t } = useI18n();
  const m = entry.measurement;
  const before = m.baseline?.low1PctFps ?? null;
  const after = m.optimized?.low1PctFps ?? null;
  const noise = formatNumber(m.noiseFps ?? 3);
  const game = entry.gameProcessName ? displayGameName(entry.gameProcessName) : null;
  const delta = formatSignedFps(m.deltaLow1PctFps);
  const verdict = m.verdict ?? "incomplete";
  const tone = verdict === "better" ? "text-ok" : verdict === "worse" ? "text-warn" : "text-fg-2";
  const sentence =
    verdict === "better"
      ? t("dashboard.d3.fps.better", { delta })
      : verdict === "worse"
        ? t("dashboard.d3.fps.worse", { delta })
        : t("dashboard.d3.fps.same", { noise });

  return (
    <Card label={t("dashboard.d3.lastOptimization")} aside={formatDateTime(entry.measuredAt)} className="flex min-h-[248px] flex-col">
      <div className="flex flex-1 flex-col">
        <p className="text-xs text-fg-3">
          {t("dashboard.d3.fps.label")}
          {game && <> · {t("dashboard.d3.fps.game", { game })}</>}
        </p>
        <div className="num mt-1 flex flex-wrap items-baseline gap-x-3 whitespace-nowrap">
          <span className="text-[34px] font-semibold text-fg-3">{fpsNumber(before)}</span>
          <ArrowRight className="h-5 w-5 self-center text-fg-3" aria-hidden />
          <span className={cn("text-[34px] font-semibold", verdict === "better" ? "text-ok" : "text-fg")}>{fpsNumber(after)}</span>
          <span className="text-sm text-fg-3">fps</span>
          {verdict !== "no_measurable_difference" && <span className={cn("text-sm font-semibold", tone)}>{delta}</span>}
        </div>
        <p className={cn("mt-1 text-[13px]", tone)}>{sentence}</p>
        {m.baseline?.avgFps != null && m.optimized?.avgFps != null && (
          <p className="text-[13px] text-fg-3">
            {t("dashboard.d3.fps.average", { before: fpsNumber(m.baseline.avgFps), after: `${fpsNumber(m.optimized.avgFps)} fps` })}
          </p>
        )}
        <div className="mt-auto flex flex-col gap-1 pt-4">
          {note}
          <p className="text-xs text-fg-3">
            {m.noiseFromWindows ? t("dashboard.d3.fps.method", { noise }) : t("dashboard.d3.fps.methodFloor", { noise })}
          </p>
        </div>
      </div>
    </Card>
  );
}
