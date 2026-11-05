import { Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useI18n } from "@/i18n";
import type { NetworkTuneSession } from "@/services/tauri/agent";

const TUNE_CONFIRM_WINDOW_S = 30;

/**
 * A janela de 30 s da otimização de TCP, a mesma que já existia: mantém ou
 * desfaz; sem resposta, desfaz sozinha. Nas que exigem reiniciar, só pede o
 * reinício (o contador começa quando o app abrir de novo).
 */
export function TuneDialog({
  session,
  secondsLeft,
  busy,
  onKeep,
  onUndo,
  onRestart,
}: {
  session: NetworkTuneSession;
  secondsLeft: number | null;
  busy: boolean;
  onKeep: () => void;
  onUndo: () => void;
  onRestart: () => void;
}) {
  const { t } = useI18n();
  const awaitingReboot = session.requiresReboot && !session.countdownStarted;
  const seconds = secondsLeft ?? 0;
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4">
      <section
        role="alertdialog"
        aria-modal="true"
        aria-label={t("mypc.network.advanced.tcp.dialogTitle")}
        className="w-full max-w-md rounded-card border border-line bg-surface p-6"
      >
        <div className="label-mono flex items-center gap-2 text-accent">
          <Wrench className="h-3.5 w-3.5" aria-hidden />
          {session.label}
        </div>
        {awaitingReboot ? (
          <>
            <p className="mt-3 text-sm leading-relaxed text-fg">{t("mypc.network.advanced.tcp.pendingReboot")}</p>
            <div className="mt-6 flex justify-end">
              <Button variant="outline" disabled={busy} onClick={onRestart}>
                {t("mypc.network.advanced.tcp.restartNow")}
              </Button>
            </div>
          </>
        ) : (
          <>
            <p className="mt-3 text-sm leading-relaxed text-fg">
              {t("mypc.network.advanced.tcp.confirmPrompt", { seconds })}
            </p>
            <Progress
              className="mt-4"
              tone="warn"
              label={t("mypc.network.advanced.tcp.confirmPrompt", { seconds })}
              value={(seconds / TUNE_CONFIRM_WINDOW_S) * 100}
            />
            <div className="mt-6 flex justify-end gap-2">
              <Button variant="outline" disabled={busy} onClick={onUndo}>
                {t("mypc.network.advanced.tcp.undo")}
              </Button>
              <Button variant="outline" disabled={busy} onClick={onKeep} autoFocus>
                {t("mypc.network.advanced.tcp.keep")}
              </Button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
