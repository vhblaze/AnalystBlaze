import { Check, Gamepad2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { SummaryRow } from "@/components/ui/summary-row";
import { StatusPill } from "@/components/ui/status-pill";
import { useI18n } from "@/i18n";
import { formatDateTime, formatMinutes } from "@/lib/format";
import type { AgentStatus } from "@/services/tauri/agent";
import { useAutomationPolicy } from "@/features/automations/useAutomationPolicy";
import { useGameMode } from "@/features/gamemode/useGameMode";
import { usePlan } from "@/features/plan/usePlan";
import type { ControlsActions } from "./actions";
import { useActionRunner } from "./useActionRunner";

/**
 * Controles › Modo Jogo. Diz o que ele faz de verdade (doc 14 H1: baixa a
 * prioridade dos apps em segundo plano; não fecha nenhum), quanto do limite
 * semanal do Starter sobrou e como voltar se algo ficou para trás.
 */
export function GameMode({ status, actions }: { status: AgentStatus | null; actions: ControlsActions }) {
  const { t } = useI18n();
  const plan = usePlan(status);
  const gameMode = useGameMode({ trackUsage: !plan.paidGameMode });
  const { policy } = useAutomationPolicy();
  const { run, busyKey } = useActionRunner();
  const session = gameMode.session;
  const usage = gameMode.usage;
  const refresh = [() => gameMode.refreshSession(), () => (plan.paidGameMode ? Promise.resolve() : gameMode.refreshUsage())];
  const ready = Boolean(status?.authenticated && status.registered);

  const toggle = () =>
    void run("toggle", session ? actions.onRestoreGameMode : actions.onActivateGameMode, {
      done: session ? t("controls.d3.gamemode.turnedOff") : t("controls.d3.gamemode.turnedOn"),
      refresh,
    });

  const does = ["priority", "background", "power", "cleanup", "network"] as const;

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("controls.d3.gamemode.title")}</h2>
          <p className="mt-1 text-sm text-fg-3">
            {session
              ? session.targetProcessName
                ? t("controls.d3.gamemode.onFor", { game: session.targetProcessName, since: formatDateTime(session.createdAt) })
                : t("controls.d3.gamemode.onSince", { since: formatDateTime(session.createdAt) })
              : t("controls.d3.gamemode.off")}
          </p>
        </div>
        <Button
          variant={session ? "outline" : "primary"}
          className="h-11 px-5 text-[15px]"
          disabled={busyKey !== null || (!session && Boolean(usage?.limitReached))}
          disabledReason={ready ? null : t("controls.d3.needsAgent")}
          onClick={toggle}
        >
          <Gamepad2 />
          {busyKey === "toggle" ? t("controls.d3.working") : session ? t("controls.d3.gamemode.turnOff") : t("controls.d3.gamemode.turnOn")}
        </Button>
      </header>

      {!plan.paidGameMode && usage && (
        <Card padding="sm" className="flex items-center justify-between gap-4 px-5">
          <span className={usage.limitReached ? "text-sm text-warn" : "text-sm text-fg-2"}>
            {usage.limitReached
              ? t("controls.d3.gamemode.usedUp")
              : t("controls.d3.gamemode.left", { minutes: formatMinutes(usage.remainingSeconds ?? 0) })}
          </span>
          <StatusPill tone="muted">{t("controls.d3.gamemode.starter")}</StatusPill>
        </Card>
      )}

      <Card label={t("controls.d3.gamemode.whatLabel")}>
        <ul className="flex flex-col gap-2">
          {does.map((item) => (
            <li key={item} className="flex items-start gap-2 text-sm text-fg-2">
              <Check className="mt-0.5 h-4 w-4 shrink-0 text-ok" aria-hidden />
              {t(`controls.d3.gamemode.does.${item}`)}
            </li>
          ))}
        </ul>
        <p className="mt-3 text-xs text-fg-3">{t("controls.d3.gamemode.whatFoot")}</p>
      </Card>

      <Card padding="sm" className="px-5">
        <SummaryRow
          title={t("controls.d3.gamemode.autoTitle")}
          summary={
            plan.automaticGameMode
              ? policy?.auto_game_mode
                ? t("controls.d3.gamemode.autoOn")
                : t("controls.d3.gamemode.autoOff")
              : t("controls.d3.auto.proOnly")
          }
          state={policy?.auto_restore_game_mode ? t("controls.d3.gamemode.autoRestoreOn") : null}
        />
        <SummaryRow
          title={t("controls.d3.gamemode.restoreTitle")}
          summary={t("controls.d3.gamemode.restoreDesc")}
          action={
            <Button
              size="sm"
              variant="outline"
              disabled={busyKey !== null}
              onClick={() => void run("restore", actions.onRestoreGameMode, { done: t("controls.d3.gamemode.restored"), refresh })}
            >
              {busyKey === "restore" ? t("controls.d3.working") : t("controls.d3.restore")}
            </Button>
          }
        />
        <SummaryRow
          title={t("controls.d3.gamemode.dvrTitle")}
          summary={t("controls.d3.gamemode.dvrDesc")}
          action={
            <Button
              size="sm"
              variant="outline"
              disabled={busyKey !== null}
              onClick={() => void run("dvr", actions.onDisableGameDvr, { done: t("controls.d3.gamemode.dvrDone") })}
            >
              {busyKey === "dvr" ? t("controls.d3.working") : t("controls.d3.gamemode.dvrAction")}
            </Button>
          }
        />
      </Card>
    </div>
  );
}
