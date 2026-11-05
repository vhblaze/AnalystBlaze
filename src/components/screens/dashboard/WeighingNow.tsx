import { useState } from "react";
import { Button } from "@/components/ui/button";
import { SectionLabel } from "@/components/ui/section-label";
import { useI18n } from "@/i18n";
import { displayGameName, displayProcessName, formatBytes, formatMegabytes, formatMs, formatPercent } from "@/lib/format";
import type { WeighingNow as Weighing } from "@/features/telemetry/weighingNow";

export type WeighingAction = "release_memory" | "open_disk" | "open_performance" | "open_network" | "game_mode_on";

/**
 * "O que está pesando agora": o problema em uma frase, o porquê em outra
 * e o único botão primário da tela. O resultado de "Liberar memória" é o
 * medido pelo agente ("liberou 3,2 GB"), nunca uma promessa (doc 13 §4).
 */
export function WeighingNow({
  weighing,
  onAction,
  onDismiss,
  releaseMemoryBlockedReason,
  gameModeBlockedReason = null,
  busy,
  lastResult,
}: {
  weighing: Weighing;
  onAction: (action: WeighingAction) => void;
  onDismiss: (kind: Weighing["kind"]) => void;
  /** Por que "Liberar memória" não roda agora (componente de sistema ausente). */
  releaseMemoryBlockedReason: string | null;
  /** Por que o Modo Jogo não liga agora (minutos da semana acabaram). */
  gameModeBlockedReason?: string | null;
  busy: boolean;
  lastResult: string | null;
}) {
  const { t } = useI18n();
  const [pending, setPending] = useState(false);

  const run = (action: WeighingAction) => {
    setPending(true);
    Promise.resolve(onAction(action)).finally(() => setPending(false));
  };

  let title: string;
  let detail: string;
  let risk: string | null = null;
  let action: { id: WeighingAction; label: string; blockedReason?: string | null } | null = null;

  switch (weighing.kind) {
    case "game_mode":
      title = weighing.game ? t("dashboard.d3.gameModeOnFor", { game: weighing.game }) : t("dashboard.d3.gameModeOn");
      detail = t("dashboard.d3.gameModeOnDetail");
      break;
    case "game_open":
      title = t("dashboard.d3.gameOpenTitle", { game: displayGameName(weighing.game) });
      detail = t("dashboard.d3.gameOpenDetail");
      action = { id: "game_mode_on", label: t("dashboard.d3.gameMode"), blockedReason: gameModeBlockedReason };
      break;
    case "memory":
      title = t("dashboard.d3.memoryTitle", { percent: formatPercent(weighing.percent) });
      detail = weighing.topProcess
        ? t("dashboard.d3.memoryDetailTop", { name: displayProcessName(weighing.topProcess.name), size: formatMegabytes(weighing.topProcess.memoryMb) })
        : t("dashboard.d3.memoryDetail");
      risk = t("dashboard.d3.memoryRisk");
      action = { id: "release_memory", label: t("dashboard.d3.releaseMemory"), blockedReason: releaseMemoryBlockedReason };
      break;
    case "disk":
      title = t("dashboard.d3.diskTitle", { drive: weighing.mountPoint.replace(/\\$/, ""), free: formatBytes(weighing.freeBytes) });
      detail = t("dashboard.d3.diskDetail");
      action = { id: "open_disk", label: t("dashboard.d3.openDisk") };
      break;
    case "cpu":
      title = t("dashboard.d3.cpuTitle", { percent: formatPercent(weighing.percent) });
      detail = t("dashboard.d3.cpuDetail");
      action = { id: "open_performance", label: t("dashboard.d3.openPerformance") };
      break;
    case "network":
      title = t("dashboard.d3.networkTitle", { latency: formatMs(weighing.latencyMs) });
      detail = t("dashboard.d3.networkDetail");
      action = { id: "open_network", label: t("dashboard.d3.openNetwork") };
      break;
    default:
      title = t("dashboard.d3.okTitle");
      detail = t("dashboard.d3.okDetail");
  }

  return (
    <div className="min-w-0 flex-1">
      <SectionLabel className={weighing.kind === "ok" ? "text-ok" : "text-warn"}>{t("dashboard.d3.weighingLabel")}</SectionLabel>
      <h2 className="mt-2 text-[24px] font-semibold leading-tight tracking-tight text-fg">{title}</h2>
      <p className="mt-2 text-[15px] text-fg-2">{detail}</p>
      {lastResult && <p className="mt-2 text-sm font-medium text-ok">{lastResult}</p>}
      {action && (
        <div className="mt-5 flex flex-wrap items-center gap-x-6 gap-y-3">
          <Button
            variant="primary"
            className="h-11 px-6 text-[15px]"
            disabled={busy || pending}
            disabledReason={action.blockedReason ?? null}
            onClick={() => run(action.id)}
          >
            {pending ? t("dashboard.d3.working") : action.label}
          </Button>
          <button
            type="button"
            onClick={() => onDismiss(weighing.kind)}
            className="text-[15px] text-fg-2 transition-colors hover:text-fg"
          >
            {t("dashboard.d3.notNow")}
          </button>
          {risk && <span className="text-[13px] text-fg-3">{risk}</span>}
          {action.blockedReason && <span className="text-[13px] text-warn">{action.blockedReason}</span>}
        </div>
      )}
    </div>
  );
}
