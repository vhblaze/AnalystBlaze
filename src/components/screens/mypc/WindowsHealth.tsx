import { useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { useI18n } from "@/i18n";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { AgentTelemetrySnapshot, SystemFileCheckStatus } from "@/services/tauri/agent";
import { useHelper } from "@/features/helper/useHelper";
import { startRepair, useSystemRepair, type RepairKind, type RepairState } from "@/features/health/useSystemRepair";
import { windowsHealthItems, type HealthItem } from "@/features/health/windowsDiagnostics";

function outcomeTone(status: SystemFileCheckStatus | null): "ok" | "warn" | "danger" {
  if (!status?.result) return "ok";
  if (status.result.success) return "ok";
  return status.result.outcome === "needs_dism_repair" ? "warn" : "danger";
}

const DOT = { danger: "bg-danger", warn: "bg-warn", ok: "bg-ok" } as const;

/**
 * Meu PC › Saúde do Windows (board Tela-MeuPC-Saude): o que está errado no
 * Windows e não é desempenho, cada item com uma ação, e o Reparo do Windows
 * (SFC e DISM) com o progresso real. Único lugar desses diagnósticos.
 */
export function WindowsHealth({
  telemetry,
  shadowConsentNeeded,
  onResolveShadowConsent,
  onRestartWindows,
  onRestartDevice,
  onStartSfc,
  onStartDism,
  onOpenInsights,
}: {
  telemetry: AgentTelemetrySnapshot | null;
  shadowConsentNeeded: boolean;
  onResolveShadowConsent: (choice: "auto" | "manual") => Promise<void>;
  onRestartWindows: () => Promise<unknown>;
  onRestartDevice: (deviceId: string) => Promise<unknown>;
  onStartSfc: () => Promise<unknown>;
  onStartDism: () => Promise<unknown>;
  onOpenInsights: () => void;
}) {
  const { t } = useI18n();
  const repair = useSystemRepair();
  const helper = useHelper();
  const [busy, setBusy] = useState<string | null>(null);
  const blocked = helper.status && !helper.ready ? t("controls.d3.needsHelper") : null;
  const items = useMemo(() => windowsHealthItems({ telemetry, shadowConsentNeeded }), [telemetry, shadowConsentNeeded]);
  const firstDanger = items.find((item) => item.severity === "danger")?.id ?? null;

  const run = async (key: string, action: () => Promise<unknown>) => {
    setBusy(key);
    try {
      await action();
    } finally {
      setBusy(null);
    }
  };

  const actionFor = (item: HealthItem) => {
    const action = item.action;
    if (!action) return <span className="text-xs text-ok">{t("mypc.health.ok")}</span>;
    const variant = item.id === firstDanger ? "primary" : "outline";
    switch (action.kind) {
      case "restart_windows":
        return (
          <Button size="sm" variant={variant} disabled={busy !== null} onClick={() => void run(item.id, onRestartWindows)}>
            {t("mypc.health.actions.restart")}
          </Button>
        );
      case "system_file_check":
        return (
          <Button
            size="sm"
            variant={variant}
            disabled={busy !== null || Boolean(repair.sfc.scanId) || repair.sfc.starting}
            disabledReason={blocked}
            onClick={() => void startRepair("sfc", onStartSfc)}
          >
            {t("mypc.health.actions.sfc")}
          </Button>
        );
      case "restart_device":
        return (
          <Button size="sm" variant={variant} disabled={busy !== null} onClick={() => void run(item.id, () => onRestartDevice(action.deviceId))}>
            {t("mypc.health.actions.restartDevice")}
          </Button>
        );
      case "shadow_consent":
        return (
          <div className="flex gap-2">
            <Button size="sm" variant={variant} disabled={busy !== null} onClick={() => void run(item.id, () => onResolveShadowConsent("auto"))}>
              {t("shadowStorage.cardAuto")}
            </Button>
            <Button size="sm" variant="link" disabled={busy !== null} onClick={() => void run(item.id, () => onResolveShadowConsent("manual"))}>
              {t("shadowStorage.cardManual")}
            </Button>
          </div>
        );
    }
  };

  const repairRow = (kind: RepairKind, state: RepairState, start: () => Promise<unknown>) => {
    const running = Boolean(state.scanId) || state.starting;
    const status = state.status;
    const outcomeKey = status?.result ? `controls.d3.repair.outcome.${status.result.outcome}` : null;
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
            {state.starting ? t("controls.d3.repair.starting") : t("controls.d3.repair.running", { elapsed: formatDuration(status?.elapsedSeconds ?? 0) })}
          </p>
        )}
        {state.error && <p className="mt-2 text-[13px] text-danger">{state.error}</p>}
        {status?.done && status.result && outcomeKey && (
          <div className="mt-2">
            <p
              className={cn(
                "text-[13px] font-medium",
                outcomeTone(status) === "ok" ? "text-ok" : outcomeTone(status) === "warn" ? "text-warn" : "text-danger",
              )}
            >
              {t(outcomeKey) === outcomeKey ? t("controls.d3.repair.outcome.unknown") : t(outcomeKey)}
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
      <p className="text-sm text-fg-2">{t("mypc.health.intro")}</p>

      <Card padding="sm" className="px-5">
        {items.length === 0 ? (
          <p className="py-4 text-sm text-fg-3">{telemetry ? t("mypc.health.noAdvanced") : t("mypc.health.noReading")}</p>
        ) : (
          <ul>
            {items.map((item) => (
              <li key={item.id} className="flex items-center gap-4 border-b border-line-soft py-3.5 last:border-b-0">
                <span aria-hidden className={cn("h-2 w-2 shrink-0 rounded-pill", DOT[item.severity])} />
                <div className="min-w-0 flex-1">
                  <div className="text-[15px] font-semibold text-fg">{t(`mypc.health.items.${item.titleKey}`, item.titleParams)}</div>
                  <div className="mt-0.5 text-[13px] text-fg-3">{t(`mypc.health.items.${item.detailKey}`, item.detailParams)}</div>
                </div>
                <div className="shrink-0">{actionFor(item)}</div>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Card label={t("controls.d3.repair.label")}>
        {repairRow("sfc", repair.sfc, onStartSfc)}
        {repairRow("dism", repair.dism, onStartDism)}
        <p className="mt-3 text-xs text-fg-3">{t("controls.d3.repair.foot")}</p>
      </Card>

      <p className="text-xs text-fg-3">
        {t("mypc.health.foot")}{" "}
        <button type="button" onClick={onOpenInsights} className="text-accent-2 underline underline-offset-2 hover:text-accent">
          {t("mypc.health.openInsights")}
        </button>
      </p>
    </div>
  );
}
