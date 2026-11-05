import { ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { useI18n } from "@/i18n";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { SystemFileCheckStatus } from "@/services/tauri/agent";
import { useHelper } from "@/features/helper/useHelper";
import { startRepair, useSystemRepair, type RepairKind, type RepairState } from "@/features/health/useSystemRepair";

function outcomeTone(status: SystemFileCheckStatus | null): "ok" | "warn" | "danger" {
  if (!status?.result) return "ok";
  if (status.result.success) return "ok";
  return status.result.outcome === "needs_dism_repair" ? "warn" : "danger";
}

/**
 * Meu PC › Saúde do Windows. Etapa 4: o Reparo do Windows (SFC e DISM)
 * mora aqui, único lugar dele (doc 14 §1.3), com o progresso real. Os
 * demais avisos do Windows chegam a esta aba na etapa 5.
 */
export function WindowsHealth({
  onStartSfc,
  onStartDism,
  onOpenInsights,
}: {
  onStartSfc: () => Promise<unknown>;
  onStartDism: () => Promise<unknown>;
  onOpenInsights: () => void;
}) {
  const { t } = useI18n();
  const repair = useSystemRepair();
  const helper = useHelper();
  const blocked = helper.status && !helper.ready ? t("controls.d3.needsHelper") : null;

  const row = (kind: RepairKind, state: RepairState, start: () => Promise<unknown>) => {
    const running = Boolean(state.scanId) || state.starting;
    const status = state.status;
    return (
      <div className="border-b border-line-soft py-3.5 last:border-b-0">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="text-[15px] font-semibold text-fg">{t(`controls.d3.repair.${kind}.title`)}</div>
            <div className="mt-0.5 text-[13px] text-fg-3">{t(`controls.d3.repair.${kind}.desc`)}</div>
          </div>
          <Button size="sm" variant="outline" disabled={running} disabledReason={blocked} onClick={() => void startRepair(kind, start)}>
            {running ? t("controls.d3.working") : t(`controls.d3.repair.${kind}.action`)}
          </Button>
        </div>
        {running && (
          <p className="mt-2 text-[13px] text-accent">
            {state.starting
              ? t("controls.d3.repair.starting")
              : t("controls.d3.repair.running", { elapsed: formatDuration(status?.elapsedSeconds ?? 0) })}
          </p>
        )}
        {state.error && <p className="mt-2 text-[13px] text-danger">{state.error}</p>}
        {status?.done && status.result && (
          <div className="mt-2">
            <p
              className={cn(
                "text-[13px] font-medium",
                outcomeTone(status) === "ok" ? "text-ok" : outcomeTone(status) === "warn" ? "text-warn" : "text-danger",
              )}
            >
              {t(`controls.d3.repair.outcome.${status.result.outcome}`) === `controls.d3.repair.outcome.${status.result.outcome}`
                ? t("controls.d3.repair.outcome.unknown")
                : t(`controls.d3.repair.outcome.${status.result.outcome}`)}
            </p>
            {status.result.outcome === "unknown" && status.result.rawTail && (
              <pre className="mt-2 max-h-32 overflow-auto rounded-control border border-line bg-bg/60 p-2 text-[11px] text-fg-3">
                {status.result.rawTail}
              </pre>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-5">
      <Card label={t("controls.d3.repair.label")}>
        {row("sfc", repair.sfc, onStartSfc)}
        {row("dism", repair.dism, onStartDism)}
        <p className="mt-3 text-xs text-fg-3">{t("controls.d3.repair.foot")}</p>
      </Card>
      <EmptyState
        icon={<ShieldCheck />}
        title={t("mypc.healthPendingTitle")}
        reason={t("mypc.healthPendingReason")}
        action={
          <Button variant="outline" onClick={onOpenInsights}>
            {t("mypc.healthOpenInsights")}
          </Button>
        }
      />
    </div>
  );
}
