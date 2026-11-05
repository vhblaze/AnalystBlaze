import { useMemo, useState } from "react";
import { ChevronDown, Gamepad2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { User } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatMegabytes, formatMinutes } from "@/lib/format";
import { route, type Route } from "@/lib/routes";
import type { AgentStatus, AgentTelemetrySnapshot, OptimizationResult, ReleaseMemoryDetails } from "@/services/tauri/agent";
import { useDiskVolumes } from "@/features/disk/useDiskVolumes";
import { useGameMode } from "@/features/gamemode/useGameMode";
import { useHelper } from "@/features/helper/useHelper";
import { performanceReport } from "@/features/performance/usePerformanceScan";
import { usePlan } from "@/features/plan/usePlan";
import { pickWeighingNow, systemVolume, type WeighingNow as Weighing } from "@/features/telemetry/weighingNow";
import { DoneToday } from "./DoneToday";
import { HealthRing } from "./HealthRing";
import { LastOptimization } from "./LastOptimization";
import { PcStrip } from "./PcStrip";
import { WeighingNow, type WeighingAction } from "./WeighingNow";

/** "Agora não" vale até fechar o app. */
const dismissedThisSession = new Set<Weighing["kind"]>();

/**
 * Dashboard D3 (board Opcao-D3-Equilibrio, doc 11 §0): (1) saúde + o que
 * está pesando agora com um único botão primário; (2) faixa do PC; (3)
 * última otimização e o que foi feito hoje. Não repete as outras telas:
 * aponta para elas.
 */
export function Dashboard({
  user,
  status,
  telemetry,
  busy,
  navigate,
  onStartAgent,
  onCollectSample,
  onActivateGameMode,
  onRestoreGameMode,
  onApplyPcCleanFast,
  onRestoreAll,
  onReleaseMemory,
}: {
  user: User | null;
  status: AgentStatus | null;
  telemetry: AgentTelemetrySnapshot | null;
  busy: boolean;
  navigate: (next: Route) => void;
  onStartAgent: () => Promise<unknown>;
  onCollectSample: () => Promise<unknown>;
  onActivateGameMode: () => Promise<unknown>;
  onRestoreGameMode: () => Promise<unknown>;
  onApplyPcCleanFast: () => Promise<unknown>;
  onRestoreAll: () => Promise<unknown>;
  /** Pede confirmação e roda; `false` = a pessoa cancelou. */
  onReleaseMemory: () => Promise<OptimizationResult | false | unknown>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("dashboard");
  const isReady = Boolean(status?.authenticated && status.registered);
  const plan = usePlan(status);
  const gameMode = useGameMode({ trackUsage: !plan.paidGameMode });
  const helper = useHelper();
  const { volumes } = useDiskVolumes({ auto: isReady });
  const [dismissed, setDismissed] = useState<Weighing["kind"][]>(() => Array.from(dismissedThisSession));
  const [measuring, setMeasuring] = useState(false);
  const [lastResult, setLastResult] = useState<string | null>(null);

  const weighing = useMemo(
    () => pickWeighingNow({ telemetry, volumes, gameSession: gameMode.session, dismissed }),
    [telemetry, volumes, gameMode.session, dismissed],
  );
  const volume = useMemo(() => systemVolume(volumes), [volumes]);

  const healthLevel = telemetry?.health_level;
  const subtitle = !isReady
    ? t("dashboard.d3.subtitleNotConnected")
    : !telemetry
      ? t("dashboard.d3.subtitleWaiting")
      : `${t(
          healthLevel === "critical"
            ? "dashboard.d3.subtitleCritical"
            : healthLevel === "watch"
              ? "dashboard.d3.subtitleWatch"
              : "dashboard.d3.subtitleGood",
        )} ${weighing.kind === "ok" ? t("dashboard.d3.subtitleNothing") : t("dashboard.d3.subtitleOneThing")}`;

  const dismiss = (kind: Weighing["kind"]) => {
    dismissedThisSession.add(kind);
    setDismissed(Array.from(dismissedThisSession));
    track("weighing_dismissed", { kind });
  };

  const onWeighingAction = async (action: WeighingAction) => {
    track("weighing_action", { action });
    if (action === "open_disk") return navigate(route("mypc", "disk"));
    if (action === "open_performance") return navigate(route("mypc", "performance"));
    if (action === "open_network") return navigate(route("mypc", "network"));
    setLastResult(null);
    const result = await onReleaseMemory();
    if (!result || typeof result !== "object") return;
    const outcome = result as OptimizationResult;
    if (!outcome.success) {
      setLastResult(null);
      return;
    }
    const freed = (outcome.details as ReleaseMemoryDetails | null)?.freed_mb;
    setLastResult(
      typeof freed === "number" && freed > 0
        ? t("dashboard.d3.releasedMeasured", { size: formatMegabytes(freed) })
        : t("dashboard.d3.releasedNoGain"),
    );
    await onCollectSample().catch(() => undefined);
  };

  const measureHealth = async () => {
    setMeasuring(true);
    try {
      await onCollectSample();
    } finally {
      setMeasuring(false);
    }
  };

  const usage = gameMode.usage;
  const gameModeActive = Boolean(gameMode.session);
  const gameModeHint = plan.paidGameMode
    ? null
    : usage?.limitReached
      ? t("dashboard.d3.gameModeUsedUp")
      : usage?.remainingSeconds != null
        ? t("dashboard.d3.gameModeFree", { minutes: formatMinutes(usage.remainingSeconds) })
        : null;

  const toggleGameMode = async () => {
    track(gameModeActive ? "game_mode_deactivate_clicked" : "game_mode_clicked");
    if (gameModeActive) {
      await onRestoreGameMode();
      gameMode.setSession(null);
    } else {
      await onActivateGameMode();
    }
    await gameMode.refreshSession().catch(() => undefined);
    if (!plan.paidGameMode) await gameMode.refreshUsage().catch(() => undefined);
  };

  const helperBlocked = helper.status && !helper.ready ? t("dashboard.d3.needsHelper") : null;
  const notReadyReason = isReady ? null : t("dashboard.d3.needsAgent");

  return (
    <div className="flex flex-col gap-6">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-[34px] font-semibold leading-tight tracking-tight text-fg">
            {t("dashboard.greetingPrefix")}{" "}
            <span className="text-gradient-brand">{user?.name?.split(" ")[0] ?? t("dashboard.fallbackPilot")}</span>
          </h1>
          <p className="mt-1 text-[15px] text-fg-2">{subtitle}</p>
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="outline" disabled={busy} disabledReason={notReadyReason} onClick={() => void toggleGameMode()}>
            <Gamepad2 />
            {gameModeActive ? t("dashboard.d3.gameModeTurnOff") : t("dashboard.d3.gameMode")}
            {gameModeHint && !gameModeActive && (
              <span className={usage?.limitReached ? "text-xs text-warn" : "text-xs font-normal text-fg-3"}>· {gameModeHint}</span>
            )}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" variant="outline" disabled={!isReady}>
                {t("dashboard.d3.moreActions")}
                <ChevronDown />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-64 border-line bg-surface">
              <DropdownMenuItem onClick={() => void onApplyPcCleanFast()}>{t("dashboard.d3.pcCleanFast")}</DropdownMenuItem>
              <DropdownMenuItem onClick={() => void performanceReport.refresh("quick").catch(() => undefined)}>
                {t("dashboard.d3.measurePerformance")}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => void onRestoreAll()}>{t("dashboard.d3.restoreAll")}</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </header>

      <Card padding="lg" className="flex items-center gap-8">
        <HealthRing
          score={telemetry ? telemetry.health_score : null}
          reasons={telemetry?.health_reasons ?? []}
          breakdown={telemetry?.health_breakdown ?? []}
          onMeasure={() => void measureHealth()}
          measuring={measuring}
        />
        {!isReady ? (
          <EmptyState
            variant="block"
            className="flex-1 border-none p-0"
            title={status?.authenticated ? t("agent.status.hardwarePending") : t("agent.status.waitingLogin")}
            reason={t("dashboard.d3.connectReason")}
            action={
              <Button variant="primary" disabled={busy} onClick={() => void onStartAgent()}>
                {t("dashboard.startAgent")}
              </Button>
            }
          />
        ) : !telemetry ? (
          <EmptyState
            variant="block"
            className="flex-1 border-none p-0"
            title={t("dashboard.d3.firstReadingTitle")}
            reason={t("dashboard.d3.firstReadingReason")}
            action={
              <Button variant="primary" disabled={measuring} onClick={() => void measureHealth()}>
                {t("dashboard.d3.measureNow")}
              </Button>
            }
          />
        ) : (
          <WeighingNow
            weighing={weighing}
            onAction={(action) => void onWeighingAction(action)}
            onDismiss={dismiss}
            releaseMemoryBlockedReason={helperBlocked}
            busy={busy}
            lastResult={lastResult}
          />
        )}
      </Card>

      <PcStrip telemetry={telemetry} volume={volume} onOpenMyPc={() => navigate(route("mypc", "performance"))} />

      <div className="grid w-full grid-cols-[minmax(0,0.8fr)_minmax(0,1fr)] gap-5">
        <LastOptimization disabledReason={notReadyReason} />
        <DoneToday onOpenHistory={() => navigate(route("history"))} />
      </div>
    </div>
  );
}
