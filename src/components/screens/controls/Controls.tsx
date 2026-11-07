import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { SummaryRow } from "@/components/ui/summary-row";
import { useI18n } from "@/i18n";
import { formatAgo } from "@/lib/format";
import { useHistory } from "@/features/history/useHistory";
import { lastActionAt } from "@/features/history/timeline";
import { useLiveTelemetry } from "@/features/telemetry/useLiveTelemetry";
import { formatBytes, formatMinutes } from "@/lib/format";
import { route, type ControlsSection, type Route } from "@/lib/routes";
import type { AgentStatus } from "@/services/tauri/agent";
import { useAutomationPolicy } from "@/features/automations/useAutomationPolicy";
import { useCleanup } from "@/features/cleanup/useCleanup";
import { useGameMode } from "@/features/gamemode/useGameMode";
import { useEnergyDiagnostics } from "@/features/health/useEnergyDiagnostics";
import { usePlan } from "@/features/plan/usePlan";
import { useStartup } from "@/features/startup/useStartup";
import { useProtectedApps, useWindowsInventory } from "@/features/windows/useWindowsInventory";
import type { ControlsActions } from "./actions";
import { Advanced } from "./Advanced";
import { Automations } from "./Automations";
import { Cleanup } from "./Cleanup";
import { Energy, powerPlanLabelKey } from "./Energy";
import { GameMode } from "./GameMode";
import { ProtectedApps } from "./ProtectedApps";
import { Services } from "./Services";
import { Startup } from "./Startup";

type Props = {
  section: ControlsSection | null;
  status: AgentStatus | null;
  busy: boolean;
  navigate: (next: Route) => void;
  actions: ControlsActions;
};

/** Controles (board Tela-Controles) e os detalhes de cada grupo. */
export function Controls(props: Props) {
  switch (props.section) {
    case "gamemode":
      return <GameMode status={props.status} actions={props.actions} />;
    case "cleanup":
      return <Cleanup actions={props.actions} />;
    case "startup":
      return <Startup actions={props.actions} />;
    case "services":
      return <Services actions={props.actions} />;
    case "protected":
      return <ProtectedApps />;
    case "energy":
      return <Energy actions={props.actions} />;
    case "advanced":
      return <Advanced actions={props.actions} navigate={props.navigate} />;
    default:
      return <ControlsHome {...props} />;
  }
}

function ControlsHome({ status, navigate }: Props) {
  const { t } = useI18n();
  const plan = usePlan(status);
  const gameMode = useGameMode({ trackUsage: !plan.paidGameMode });
  const { policy } = useAutomationPolicy();
  const cleanup = useCleanup();
  const startup = useStartup();
  const inventory = useWindowsInventory();
  const protectedApps = useProtectedApps();
  const { energy } = useEnergyDiagnostics();
  const history = useHistory();
  const live = useLiveTelemetry();
  const failingServices = ((live?.advanced as { failing_services?: Array<{ name: string }> } | null | undefined)?.failing_services ?? []).length;
  const lastCleanupAt = lastActionAt(history.audit, ["APPLY_CLEANUP_CATEGORY", "EMPTY_TEMP", "APPLY_PC_CLEAN_FAST_PROFILE"]);
  const lastGameModeAt = lastActionAt(history.audit, ["APPLY_GAME_MODE"]);
  const lastRun = (at: number | null) => (at ? t("controls.d3.groups.lastRun", { when: formatAgo(at) }) : null);

  const open = (section: ControlsSection) => (
    <Button size="sm" variant="outline" onClick={() => navigate(route("controls", section))}>
      {t("controls.d3.open")}
    </Button>
  );

  const reclaimable = cleanup.categories
    .filter((category) => category.id !== "cleanup_quarantine")
    .reduce((sum, category) => sum + category.reclaimableBytes, 0);
  const cleanupCount = cleanup.categories.filter((category) => category.id !== "cleanup_quarantine" && category.reclaimableBytes > 0).length;
  const startupOn = startup.rows.filter((row) => row.state === "on");
  // null = o agente ainda não tem dados de uso (não é "nunca aberto").
  const unused = startupOn.filter((row) => row.lastSeenDaysAgo !== null && row.lastSeenDaysAgo >= 30).length;
  const suggested = startupOn.filter((row) => row.suggested && row.canChange).length;

  const usage = gameMode.usage;
  const gameModeSummary = [
    t("controls.d3.groups.gamemode.summary"),
    plan.paidGameMode
      ? null
      : usage?.limitReached
        ? t("controls.d3.groups.gamemode.usedUp")
        : usage?.remainingSeconds != null
          ? t("controls.d3.groups.gamemode.free", { minutes: formatMinutes(usage.remainingSeconds) })
          : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="grid grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)] gap-5">
      <div className="flex flex-col gap-5">
        <header>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("controls.d3.title")}</h2>
          <p className="mt-1 text-sm text-fg-3">{t("controls.d3.subtitle")}</p>
        </header>
        <Card padding="sm" className="px-5" data-tour="controls-list">
          <SummaryRow
            title={t("controls.d3.groups.gamemode.title")}
            summary={gameModeSummary}
            state={gameMode.session ? t("controls.d3.groups.gamemode.on") : policy?.auto_game_mode && plan.automaticGameMode ? t("controls.d3.groups.gamemode.auto") : lastRun(lastGameModeAt)}
            action={open("gamemode")}
          />
          <SummaryRow
            title={t("controls.d3.groups.cleanup.title")}
            summary={
              cleanup.loading && cleanup.categories.length === 0
                ? t("controls.d3.measuring")
                : reclaimable > 0
                  ? t("controls.d3.groups.cleanup.summary", { size: formatBytes(reclaimable), count: cleanupCount })
                  : t("controls.d3.groups.cleanup.nothing")
            }
            state={lastRun(lastCleanupAt)}
            action={open("cleanup")}
          />
          <SummaryRow
            title={t("controls.d3.groups.startup.title")}
            summary={
              startup.loading && startup.rows.length === 0
                ? t("controls.d3.measuring")
                : suggested > 0
                  ? t("controls.d3.groups.startup.summarySuggested", { count: startupOn.length, suggested })
                  : t("controls.d3.groups.startup.summary", { count: startupOn.length })
            }
            warn={suggested > 0}
            state={unused > 0 ? t("controls.d3.groups.startup.unused", { count: unused }) : null}
            action={open("startup")}
          />
          <SummaryRow
            title={t("controls.d3.groups.services.title")}
            summary={t("controls.d3.groups.services.summary", { count: inventory.services.length })}
            warn={failingServices > 0}
            state={failingServices > 0 ? t("controls.d3.groups.services.failing", { count: failingServices }) : null}
            action={open("services")}
          />
          <SummaryRow
            title={t("controls.d3.groups.protected.title")}
            summary={t("controls.d3.groups.protected.summary")}
            state={t("controls.d3.groups.protected.count", { count: protectedApps.apps.length })}
            action={open("protected")}
          />
          <SummaryRow
            title={t("controls.d3.groups.energy.title")}
            summary={t("controls.d3.groups.energy.summary")}
            state={energy ? t(powerPlanLabelKey(energy.active_scheme_alias)) : null}
            action={open("energy")}
          />
          <SummaryRow
            title={t("controls.d3.groups.advanced.title")}
            summary={t("controls.d3.groups.advanced.summary")}
            state={t("controls.d3.groups.advanced.state")}
            action={open("advanced")}
          />
        </Card>
      </div>
      <Automations status={status} />
    </div>
  );
}
