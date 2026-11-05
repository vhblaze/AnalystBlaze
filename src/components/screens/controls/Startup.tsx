import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmInline } from "@/components/ui/confirm-inline";
import { EmptyState } from "@/components/ui/empty-state";
import { Segmented } from "@/components/ui/segmented";
import { StatusPill } from "@/components/ui/status-pill";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { formatSecondsFromMs } from "@/lib/format";
import { disableStartupApp } from "@/services/tauri/agent";
import { useHelper } from "@/features/helper/useHelper";
import { bootDelayFor, bootPerformance, useStartup, type StartupRow } from "@/features/startup/useStartup";
import type { ControlsActions } from "./actions";
import { useActionRunner } from "./useActionRunner";

/**
 * Controles › Inicialização (board Tela-Controles-Inicializacao), com duas
 * correções de honestidade:
 * - segundos só quando o Windows mediu (lacuna L2, log de boot lido pelo
 *   componente de sistema); sem a medição, o impacto é alto/médio/baixo;
 * - sem "Adia": o "Atrasar" do agente tira o app da inicialização e não o
 *   abre depois (performance_suite.rs). Por isso cada app é Liga ou Não.
 */
export function Startup({ actions }: { actions: ControlsActions }) {
  const { t } = useI18n();
  const startup = useStartup();
  const helper = useHelper();
  const boot = bootPerformance.use({ auto: helper.ready });
  const { run, busyKey } = useActionRunner();
  const [confirmingBatch, setConfirmingBatch] = useState(false);

  const on = startup.rows.filter((row) => row.state === "on");
  const suggested = on.filter((row) => row.suggested && row.canChange);
  const refresh = [() => startup.refresh()];

  const setState = (row: StartupRow, next: "on" | "off") => {
    if (next === row.state) return;
    const key = `row:${row.name}`;
    if (next === "off") {
      void run(key, () => actions.onDisableStartup(row.name, row.location), {
        done: t("controls.d3.startup.turnedOff", { name: row.name }),
        refresh,
      });
    } else {
      const restore = row.offVia === "delay" ? actions.onRestoreDelayedStartupApp : actions.onRestoreStartup;
      void run(key, () => restore(row.name), { done: t("controls.d3.startup.turnedOn", { name: row.name }), refresh });
    }
  };

  const removeSuggested = () => {
    setConfirmingBatch(false);
    void run(
      "batch",
      async () => {
        for (const row of suggested) {
          const result = await disableStartupApp(row.name, row.location);
          if (!result.success) return result;
        }
        return { success: true };
      },
      { done: t("controls.d3.startup.batchDone", { count: suggested.length }), refresh },
    );
  };

  const lastSeen = (row: StartupRow) => {
    if (row.state === "off") return t("controls.d3.startup.off");
    if (row.lastSeenDaysAgo === null) return t("controls.d3.startup.noData");
    if (row.lastSeenDaysAgo === 0) return t("controls.d3.startup.today");
    return t("controls.d3.startup.daysAgo", { days: row.lastSeenDaysAgo });
  };

  const bootLine = (() => {
    if (helper.status && !helper.ready) return <p className="mt-2 text-[13px] text-fg-3">{t("controls.d3.startup.boot.needsHelper")}</p>;
    if (!helper.ready) return null;
    if (boot.loading && !boot.data) return <p className="mt-2 text-[13px] text-fg-3">{t("controls.d3.startup.boot.reading")}</p>;
    if (boot.error) {
      return (
        <p className="mt-2 flex flex-wrap items-center gap-3 text-[13px] text-warn">
          {t("controls.d3.startup.boot.unavailable", { reason: boot.error })}
          <Button variant="link" size="sm" onClick={() => void boot.refresh().catch(() => undefined)}>
            {t("controls.d3.startup.boot.retry")}
          </Button>
        </p>
      );
    }
    const last = boot.data?.lastBoot;
    if (!last) return boot.data ? <p className="mt-2 text-[13px] text-fg-3">{t("controls.d3.startup.boot.noEvents")}</p> : null;
    const recent = boot.data?.recentBoots ?? [];
    const average = recent.length > 1 ? recent.reduce((sum, item) => sum + item.bootMs, 0) / recent.length : null;
    return (
      <p className="mt-2 text-[13px] text-fg-2">
        <span className="font-medium text-fg">{t("controls.d3.startup.boot.last", { time: formatSecondsFromMs(last.bootMs) })}</span>
        <span className="text-fg-3">
          {" · "}
          {average !== null
            ? t("controls.d3.startup.boot.lastDetail", { count: recent.length, average: formatSecondsFromMs(average) })
            : t("controls.d3.startup.boot.lastDetailSingle")}
        </span>
      </p>
    );
  })();

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">
            {t("controls.d3.startup.title", { count: on.length })}
          </h2>
          <p className="mt-1 text-sm text-fg-3">{t("controls.d3.startup.subtitle")}</p>
          {bootLine}
        </div>
        {suggested.length > 0 && !confirmingBatch && (
          <Button variant="primary" className="h-11 px-5 text-[15px]" disabled={busyKey !== null} onClick={() => setConfirmingBatch(true)}>
            {t("controls.d3.startup.removeSuggested", { count: suggested.length })}
          </Button>
        )}
      </header>

      {confirmingBatch && (
        <ConfirmInline
          message={t("controls.d3.startup.batchConfirm", { names: suggested.map((row) => row.name).join(", ") })}
          confirmLabel={t("controls.d3.startup.batchConfirmButton")}
          onConfirm={removeSuggested}
          onCancel={() => setConfirmingBatch(false)}
        />
      )}

      {startup.rows.length === 0 ? (
        <EmptyState
          title={startup.loading ? t("controls.d3.measuring") : t("controls.d3.startup.emptyTitle")}
          reason={startup.loading ? null : startup.error ?? t("controls.d3.startup.emptyReason")}
        />
      ) : (
        <Card padding="sm" className="px-5">
          <div className="grid grid-cols-[minmax(0,1fr)_130px_120px_150px] gap-4 border-b border-line-soft py-2.5 text-xs text-fg-3">
            <span>{t("controls.d3.startup.colApp")}</span>
            <span>{t("controls.d3.startup.colImpact")}</span>
            <span>{t("controls.d3.startup.colOpened")}</span>
            <span>{t("controls.d3.startup.colAtBoot")}</span>
          </div>
          <ul>
            {startup.rows.map((row) => (
              <li
                key={`${row.state}:${row.name}`}
                className="grid grid-cols-[minmax(0,1fr)_130px_120px_150px] items-center gap-4 border-b border-line-soft py-3 last:border-b-0"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-[15px] font-medium text-fg">{row.name}</span>
                    {row.state === "on" && row.suggested && row.canChange && (
                      <StatusPill tone="warn">{t("controls.d3.startup.suggested")}</StatusPill>
                    )}
                  </div>
                  <div className="truncate text-xs text-fg-3">{row.publisher ?? row.location ?? t("controls.d3.startup.removedByUs")}</div>
                </div>
                {(() => {
                  const delayMs = row.state === "on" ? bootDelayFor(row, boot.data) : null;
                  if (delayMs !== null) {
                    const time = formatSecondsFromMs(delayMs);
                    return (
                      <span className="text-sm text-warn" title={t("controls.d3.startup.boot.delayedHint", { time })}>
                        {t("controls.d3.startup.boot.delayed", { time })}
                      </span>
                    );
                  }
                  return (
                    <span className={cn("text-sm", row.impact === "high" ? "text-warn" : "text-fg-2")}>
                      {row.impact ? t(`controls.d3.startup.impact.${row.impact}`) : t("controls.d3.startup.impact.unknown")}
                    </span>
                  );
                })()}
                <span className="text-sm text-fg-2">{lastSeen(row)}</span>
                <Segmented<"on" | "off">
                  size="sm"
                  aria-label={t("controls.d3.startup.colAtBoot")}
                  value={row.state}
                  onChange={(next) => setState(row, next)}
                  options={[
                    { value: "on", label: t("controls.d3.startup.on"), disabled: !row.canChange || busyKey !== null },
                    { value: "off", label: t("controls.d3.startup.offOption"), disabled: !row.canChange || busyKey !== null },
                  ]}
                />
              </li>
            ))}
          </ul>
        </Card>
      )}

      <p className="text-xs text-fg-3">{t("controls.d3.startup.footnote")}</p>
    </div>
  );
}
