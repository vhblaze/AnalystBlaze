import { useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { ConfirmInline } from "@/components/ui/confirm-inline";
import { StatusPill } from "@/components/ui/status-pill";
import { SummaryRow } from "@/components/ui/summary-row";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatBytes, formatDateTime, formatScore } from "@/lib/format";
import { route, type Route } from "@/lib/routes";
import {
  getAuditLog,
  installPrivilegedHelper,
  purgeCleanupQuarantine,
  restartPrivilegedHelper,
  startPrivilegedHelper,
  stopPrivilegedHelper,
  testPrivilegedHelper,
  uninstallPrivilegedHelper,
  type PrivilegedHelperStatus,
} from "@/services/tauri/agent";
import { useCleanup } from "@/features/cleanup/useCleanup";
import { privilegedHelperStatus, useHelper } from "@/features/helper/useHelper";
import { useHistory } from "@/features/history/useHistory";
import { usePerformanceScan } from "@/features/performance/usePerformanceScan";
import { errorText } from "@/features/shared/agentResource";
import type { ControlsActions } from "./actions";
import { useActionRunner } from "./useActionRunner";

const BREAKDOWN = ["bootStartup", "background", "memory", "disk", "network", "thermal"] as const;

/**
 * Controles › Avançado (board Tela-Controles-Avancado): tudo o que existe
 * hoje, reorganizado. Sem as categorias de limpeza (estão em Limpeza), sem
 * serviços (estão em Serviços) e sem SFC/DISM (estão em Meu PC › Saúde):
 * cada função mora num lugar só (doc 14 §1.3).
 */
export function Advanced({ actions, navigate }: { actions: ControlsActions; navigate: (next: Route) => void }) {
  return (
    <div className="grid grid-cols-2 items-start gap-5">
      <div className="flex flex-col gap-5">
        <Measurement actions={actions} />
        <SystemComponent />
      </div>
      <div className="flex flex-col gap-5">
        <Quarantine />
        <RevertAndLogs actions={actions} navigate={navigate} />
      </div>
    </div>
  );
}

function Measurement({ actions }: { actions: ControlsActions }) {
  const { t } = useI18n();
  const scan = usePerformanceScan({ auto: false });
  const { run, busyKey } = useActionRunner();
  const report = scan.report;
  const measure = (mode: "baseline" | "after") => void scan.refresh(mode).catch(() => undefined);
  const session = report?.restoreSession?.status === "available" ? report.restoreSession : null;

  return (
    <Card label={t("controls.d3.advanced.measureLabel")} aside={report ? formatDateTime(report.generatedAt) : null}>
      {report ? (
        <div className="num flex items-baseline gap-3 text-sm text-fg-3">
          {report.previousScore != null && (
            <>
              <span>{t("controls.d3.advanced.before")}</span>
              <span className="text-2xl font-semibold text-fg">{Math.round(report.previousScore)}</span>
              <ArrowRight className="h-4 w-4 self-center" aria-hidden />
              <span>{t("controls.d3.advanced.after")}</span>
            </>
          )}
          <span className="text-2xl font-semibold text-ok">{Math.round(report.overallScore)}</span>
        </div>
      ) : (
        <p className="text-sm text-fg-3">{scan.loading ? t("controls.d3.measuring") : scan.error ?? t("controls.d3.advanced.noMeasure")}</p>
      )}

      {report && (
        <ul className="mt-3">
          {report.bottlenecks.slice(0, 3).map((item) => (
            <li key={item.id} className="flex items-start justify-between gap-3 border-b border-line-soft py-2 text-sm last:border-b-0">
              <span className="text-fg">
                {item.label}
                {item.hardwareContext && <span className="block text-xs text-fg-3">{item.hardwareContext}</span>}
              </span>
              <StatusPill tone={item.severity === "high" ? "warn" : "muted"}>{t(`controls.d3.advanced.severity.${item.severity === "high" ? "high" : item.severity === "medium" ? "medium" : "low"}`)}</StatusPill>
            </li>
          ))}
          {report.bottlenecks.length === 0 && <li className="py-2 text-sm text-fg-3">{t("controls.d3.advanced.noBottleneck")}</li>}
        </ul>
      )}

      {report && (
        <dl className="mt-3 grid grid-cols-3 gap-x-4 gap-y-1.5 text-xs">
          {BREAKDOWN.map((key) => (
            <div key={key} className="flex justify-between gap-2">
              <dt className="text-fg-3">{t(`controls.d3.advanced.breakdown.${key}`)}</dt>
              <dd className="num text-fg-2">{formatScore(report.scoreBreakdown[key])}</dd>
            </div>
          ))}
        </dl>
      )}

      <div className="mt-4 flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={scan.loading} onClick={() => measure("baseline")}>
          {t("controls.d3.advanced.measureBefore")}
        </Button>
        <Button size="sm" variant="outline" disabled={scan.loading} onClick={() => measure("after")}>
          {t("controls.d3.advanced.measureAfter")}
        </Button>
        {session && (
          <Button
            size="sm"
            variant="outline"
            disabled={busyKey !== null}
            onClick={() =>
              void run("session", () => actions.onRestorePerformanceSession(session.id), {
                done: t("controls.d3.advanced.sessionRestored"),
                refresh: [() => scan.refresh("after")],
              })
            }
          >
            {t("controls.d3.advanced.undoSession")}
          </Button>
        )}
      </div>
      <p className="mt-3 text-xs text-fg-3">{t("controls.d3.advanced.measureFoot")}</p>
    </Card>
  );
}

function helperStateKey(status: PrivilegedHelperStatus | null): string {
  if (!status) return "unknown";
  if (!status.installed) return "notInstalled";
  if (!status.running) return "stopped";
  if (status.requiresUpdate) return "outdated";
  return status.available ? "running" : "attention";
}

function SystemComponent() {
  const { t } = useI18n();
  const track = useTelemetry("controls_helper");
  const helper = useHelper();
  const [busy, setBusy] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const status = helper.status;
  const state = helperStateKey(status);

  const act = async (key: string, action: () => Promise<PrivilegedHelperStatus>, done: string) => {
    setBusy(key);
    try {
      privilegedHelperStatus.set(await action());
      toast({ title: done });
    } catch (error) {
      toast({ title: t("controls.d3.actionFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setBusy(null);
    }
  };

  const test = async () => {
    setBusy("test");
    try {
      const handshake = await testPrivilegedHelper();
      await privilegedHelperStatus.refresh().catch(() => undefined);
      track("privileged_helper_tested", { ok: handshake.ok, latencyMs: handshake.latencyMs });
      toast({ title: handshake.ok ? t("controls.d3.helper.testOk") : t("controls.d3.helper.testFailed"), description: handshake.message, variant: handshake.ok ? undefined : "destructive" });
    } catch (error) {
      toast({ title: t("controls.d3.helper.testFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setBusy(null);
    }
  };

  const perMachineReason = status?.canRequestUac === false ? t("settings.helperNeedsPerMachine") : null;

  return (
    <Card label={t("controls.d3.helper.label")}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-[15px] font-semibold text-fg">{t(`controls.d3.helper.state.${state}`)}</div>
          <div className="text-xs text-fg-3">
            {status?.version ? t("controls.d3.helper.version", { version: status.version }) : t("controls.d3.helper.needed")}
          </div>
        </div>
        <Button size="sm" variant="outline" disabled={busy !== null || !status?.installed} onClick={() => void test()}>
          {busy === "test" ? t("controls.d3.working") : t("controls.d3.helper.test")}
        </Button>
      </div>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={busy !== null}
          disabledReason={perMachineReason}
          onClick={() =>
            void act("install", installPrivilegedHelper, status?.installed ? t("settings.helperRepaired") : t("settings.helperInstalledMsg"))
          }
        >
          {status?.installed ? t("controls.d3.helper.repair") : t("controls.d3.helper.install")}
        </Button>
        {status?.installed && !status.running && (
          <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => void act("start", startPrivilegedHelper, t("settings.helperStarted"))}>
            {t("controls.d3.helper.start")}
          </Button>
        )}
        {status?.installed && status.running && (
          <>
            <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => void act("restart", restartPrivilegedHelper, t("settings.helperRestarted"))}>
              {t("controls.d3.helper.restart")}
            </Button>
            <Button size="sm" variant="outline" disabled={busy !== null} onClick={() => void act("stop", stopPrivilegedHelper, t("settings.helperStopped"))}>
              {t("controls.d3.helper.stop")}
            </Button>
          </>
        )}
        {status?.installed && !confirmRemove && (
          <Button size="sm" variant="danger" disabled={busy !== null} onClick={() => setConfirmRemove(true)}>
            {t("controls.d3.helper.remove")}
          </Button>
        )}
      </div>
      {confirmRemove && (
        <ConfirmInline
          className="mt-3"
          danger
          message={t("controls.d3.helper.removeConfirm")}
          confirmLabel={t("controls.d3.helper.remove")}
          busy={busy !== null}
          onCancel={() => setConfirmRemove(false)}
          onConfirm={() => {
            setConfirmRemove(false);
            void act("remove", uninstallPrivilegedHelper, t("settings.helperRemoved"));
          }}
        />
      )}
      <p className="mt-3 text-xs text-fg-3">{t("controls.d3.helper.foot")}</p>
    </Card>
  );
}

function Quarantine() {
  const { t } = useI18n();
  const cleanup = useCleanup();
  const { run, busyKey } = useActionRunner();
  const [confirming, setConfirming] = useState(false);
  const quarantine = cleanup.categories.find((category) => category.id === "cleanup_quarantine");
  const size = quarantine?.reclaimableBytes ?? 0;

  return (
    <Card label={t("controls.d3.quarantine.label")}>
      <SummaryRow
        className="py-0"
        title={t("controls.d3.quarantine.title")}
        summary={size > 0 ? t("controls.d3.quarantine.size", { size: formatBytes(size) }) : t("controls.d3.quarantine.empty")}
        action={
          !confirming && (
            <Button size="sm" variant="danger" disabled={busyKey !== null || size <= 0} onClick={() => setConfirming(true)}>
              {busyKey === "purge" ? t("controls.d3.working") : t("controls.d3.quarantine.purge")}
            </Button>
          )
        }
      />
      {confirming && (
        <ConfirmInline
          className="mt-3"
          danger
          message={t("controls.d3.quarantine.confirm", { size: formatBytes(size) })}
          confirmLabel={t("controls.d3.quarantine.purge")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            void run("purge", purgeCleanupQuarantine, {
              done: t("controls.d3.quarantine.done"),
              refresh: [() => cleanup.refresh()],
            });
          }}
        />
      )}
      <p className="mt-3 text-xs text-fg-3">{t("controls.d3.quarantine.foot")}</p>
    </Card>
  );
}

function RevertAndLogs({ actions, navigate }: { actions: ControlsActions; navigate: (next: Route) => void }) {
  const { t } = useI18n();
  const history = useHistory();
  const { run, busyKey } = useActionRunner();
  const pending = history.snapshots.filter((snapshot) => !snapshot.restored_at).length;

  const exportAudit = async () => {
    try {
      const events = await getAuditLog(1000);
      const blob = new Blob([JSON.stringify(events, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `analystblaze-auditoria-${new Date().toISOString().slice(0, 10)}.json`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (error) {
      toast({ title: t("controls.d3.actionFailed"), description: errorText(error), variant: "destructive" });
    }
  };

  return (
    <Card label={t("controls.d3.revert.label")} padding="md">
      <SummaryRow
        title={t("controls.d3.revert.allTitle")}
        summary={t("controls.d3.revert.allSummary", { count: pending })}
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={busyKey !== null || pending === 0}
            onClick={() => void run("all", actions.onRestoreOptimizations, { done: t("controls.restoreCompleted") })}
          >
            {busyKey === "all" ? t("controls.d3.working") : t("controls.d3.revert.allAction")}
          </Button>
        }
      />
      <SummaryRow
        title={t("controls.d3.revert.gameTitle")}
        summary={t("controls.d3.revert.gameSummary")}
        action={
          <Button size="sm" variant="outline" disabled={busyKey !== null} onClick={() => void run("game", actions.onRestoreGameMode, { done: t("controls.d3.gamemode.restored") })}>
            {busyKey === "game" ? t("controls.d3.working") : t("controls.d3.restore")}
          </Button>
        }
      />
      <SummaryRow
        title={t("controls.d3.revert.snapshotsTitle")}
        summary={t("controls.d3.revert.snapshotsSummary", { count: history.snapshots.length })}
        action={
          <Button size="sm" variant="link" onClick={() => navigate(route("history"))}>
            {t("controls.d3.revert.see")}
          </Button>
        }
      />
      <SummaryRow
        title={t("controls.d3.revert.auditTitle")}
        summary={t("controls.d3.revert.auditSummary")}
        action={
          <Button size="sm" variant="link" onClick={() => void exportAudit()}>
            {t("controls.d3.revert.export")}
          </Button>
        }
      />
    </Card>
  );
}
