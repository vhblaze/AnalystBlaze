import { RefreshCw } from "lucide-react";
import { useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { SummaryRow } from "@/components/ui/summary-row";
import { useI18n } from "@/i18n";
import { useHistory } from "@/features/history/useHistory";
import { useWindowsInventory } from "@/features/windows/useWindowsInventory";
import type { ControlsActions } from "./actions";
import { useActionRunner } from "./useActionRunner";

/** Nomes de serviços que o AnalystBlaze parou e ainda não restaurou (snapshots ativos). */
function stoppedByUs(snapshots: ReturnType<typeof useHistory>["snapshots"]): Set<string> {
  const names = new Set<string>();
  for (const snapshot of snapshots) {
    if (snapshot.restored_at) continue;
    for (const entry of snapshot.entries) {
      const e = entry as Record<string, unknown> | null;
      if (e?.kind === "service_state" && typeof e.service_name === "string") names.add(e.service_name.toLowerCase());
    }
  }
  return names;
}

/**
 * Controles › Serviços do Windows: só os que o agente classifica como
 * seguros e permite mexer. Parar guarda o estado anterior; Restaurar devolve.
 */
export function Services({ actions }: { actions: ControlsActions }) {
  const { t } = useI18n();
  const inventory = useWindowsInventory();
  const history = useHistory();
  const { run, busyKey } = useActionRunner();
  const stopped = useMemo(() => stoppedByUs(history.snapshots), [history.snapshots]);
  const refresh = [() => inventory.refresh()];

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("controls.d3.services.title")}</h2>
          <p className="mt-1 text-sm text-fg-3">{t("controls.d3.services.subtitle")}</p>
        </div>
        <Button size="sm" variant="outline" disabled={inventory.loading || busyKey !== null} onClick={() => void inventory.refresh().catch(() => undefined)}>
          <RefreshCw />
          {t("common.refresh")}
        </Button>
      </header>
      {inventory.services.length === 0 ? (
        <EmptyState
          title={inventory.loading ? t("controls.d3.measuring") : t("controls.d3.services.emptyTitle")}
          reason={inventory.loading ? null : inventory.error ?? t("controls.d3.services.emptyReason")}
        />
      ) : (
        <Card padding="sm" className="px-5">
          {inventory.services.map((service) => {
            const isStopped = stopped.has(service.name.toLowerCase());
            const key = `svc:${service.name}`;
            return (
              <SummaryRow
                key={service.name}
                title={service.display_name || service.name}
                summary={isStopped ? t("controls.d3.services.stoppedByUs") : service.name}
                action={
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busyKey !== null}
                    onClick={() =>
                      void run(
                        key,
                        () => (isStopped ? actions.onRestoreService(service.name) : actions.onStopService(service.name)),
                        {
                          done: isStopped
                            ? t("controls.d3.services.restored", { name: service.display_name || service.name })
                            : t("controls.d3.services.stopped", { name: service.display_name || service.name }),
                          refresh,
                        },
                      )
                    }
                  >
                    {busyKey === key ? t("controls.d3.working") : isStopped ? t("controls.d3.services.restore") : t("controls.d3.services.stop")}
                  </Button>
                }
              />
            );
          })}
        </Card>
      )}
      <p className="text-xs text-fg-3">{t("controls.d3.services.footnote")}</p>
    </div>
  );
}
