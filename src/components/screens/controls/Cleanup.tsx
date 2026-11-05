import { RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { SummaryRow } from "@/components/ui/summary-row";
import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import { useCleanup } from "@/features/cleanup/useCleanup";
import { useHelper } from "@/features/helper/useHelper";
import type { ControlsActions } from "./actions";
import { useActionRunner } from "./useActionRunner";

/**
 * Controles › Limpeza: as categorias reais de scanCleanupCategories e a soma
 * calculada delas (doc 14 Q2). Tudo vai para a quarentena reversível;
 * esvaziar a quarentena fica em Controles › Avançado.
 */
export function Cleanup({ actions }: { actions: ControlsActions }) {
  const { t } = useI18n();
  const cleanup = useCleanup();
  const helper = useHelper();
  const { run, busyKey } = useActionRunner();
  const categories = cleanup.categories.filter((category) => category.id !== "cleanup_quarantine");
  const total = categories.reduce((sum, category) => sum + category.reclaimableBytes, 0);
  const refresh = [() => cleanup.refresh()];
  const helperReason = helper.status && !helper.ready ? t("controls.d3.needsHelper") : null;

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <header className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">
            {total > 0 ? t("controls.d3.cleanup.title", { size: formatBytes(total) }) : t("controls.d3.cleanup.titleNothing")}
          </h2>
          <p className="mt-1 text-sm text-fg-3">{t("controls.d3.cleanup.subtitle")}</p>
        </div>
        <Button size="sm" variant="outline" disabled={cleanup.loading} onClick={() => void cleanup.refresh().catch(() => undefined)}>
          <RefreshCw className={cleanup.loading ? "animate-spin" : undefined} />
          {t("controls.d3.measureAgain")}
        </Button>
      </header>

      {categories.length === 0 ? (
        <EmptyState
          title={cleanup.loading ? t("controls.d3.measuring") : t("controls.d3.cleanup.emptyTitle")}
          reason={cleanup.loading ? null : cleanup.error ?? t("controls.d3.cleanup.emptyReason")}
        />
      ) : (
        <Card padding="sm" className="px-5">
          {categories.map((category) => {
            const blocked = category.requiresHelper ? helperReason : null;
            return (
              <SummaryRow
                key={category.id}
                title={category.label}
                summary={[formatBytes(category.reclaimableBytes), category.skippedReason].filter(Boolean).join(" · ")}
                state={category.requiresHelper ? t("controls.d3.cleanup.usesHelper") : null}
                action={
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyKey !== null || category.reclaimableBytes <= 0}
                    disabledReason={blocked}
                    onClick={() =>
                      void run(category.id, () => actions.onApplyCleanupCategory(category.id, "safe"), {
                        done: t("controls.d3.cleanup.done", { name: category.label }),
                        refresh,
                      })
                    }
                  >
                    {busyKey === category.id ? t("controls.d3.working") : t("controls.d3.cleanup.clean")}
                  </Button>
                }
              />
            );
          })}
          <SummaryRow
            title={t("controls.d3.cleanup.deepTemp")}
            summary={t("controls.d3.cleanup.deepTempDesc")}
            action={
              <Button
                size="sm"
                variant="outline"
                disabled={busyKey !== null}
                onClick={() => void run("deep", actions.onDeepCleanTemp, { done: t("controls.d3.cleanup.deepTempDone"), refresh })}
              >
                {busyKey === "deep" ? t("controls.d3.working") : t("controls.d3.cleanup.clean")}
              </Button>
            }
          />
          <SummaryRow
            title={t("controls.d3.cleanup.pcCleanFast")}
            summary={t("controls.d3.cleanup.pcCleanFastDesc")}
            action={
              <Button
                size="sm"
                variant="outline"
                disabled={busyKey !== null}
                onClick={() => void run("pcclean", actions.onApplyPcCleanFast, { done: t("controls.d3.cleanup.pcCleanFastDone"), refresh })}
              >
                {busyKey === "pcclean" ? t("controls.d3.working") : t("controls.d3.apply")}
              </Button>
            }
          />
        </Card>
      )}
      <p className="text-xs text-fg-3">{t("controls.d3.cleanup.footnote")}</p>
    </div>
  );
}
