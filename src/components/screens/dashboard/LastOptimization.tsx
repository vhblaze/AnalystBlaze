import { ArrowRight, Gauge } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { useI18n } from "@/i18n";
import { formatDateTime, formatSignedPercent } from "@/lib/format";
import { usePerformanceScan } from "@/features/performance/usePerformanceScan";

/**
 * "Última otimização". O board mostra o 1% low antes/depois do jogo, que
 * depende da captura de frames ligada ao Modo Jogo (lacuna L4). Até lá,
 * mostra o antes/depois que existe: a Medição de desempenho, com esse nome
 * (doc 14 Q3), e esconde o gráfico.
 */
export function LastOptimization({ disabledReason }: { disabledReason: string | null }) {
  const { t } = useI18n();
  const scan = usePerformanceScan({ auto: false });
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
          <p className="mt-auto pt-4 text-xs text-fg-3">{t("dashboard.d3.fpsPending")}</p>
        </div>
      )}
    </Card>
  );
}
