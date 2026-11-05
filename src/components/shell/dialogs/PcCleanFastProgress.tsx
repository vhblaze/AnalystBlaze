import { useEffect, useState } from "react";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import { closePcCleanFastRun, usePcCleanFastRun } from "@/features/gamemode/pcCleanFastRun";

/**
 * Pop-up de "PC limpo e rápido": só abre depois do clique do usuário (e da
 * confirmação local). Enquanto o agente trabalha mostra as regras do perfil
 * e o tempo; no fim, o score medido antes e depois e o que foi feito.
 */
export function PcCleanFastProgress({ onUndo }: { onUndo: (sessionId: string | null) => void }) {
  const { t } = useI18n();
  const run = usePcCleanFastRun();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (run.phase !== "running") return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [run.phase]);

  if (run.phase === "idle") return null;

  const steps = [
    t("pcCleanFastRun.stepMeasure"),
    t("pcCleanFastRun.stepClean"),
    t("pcCleanFastRun.stepStartup"),
    t("pcCleanFastRun.stepBackground"),
    t("pcCleanFastRun.stepMeasureAgain"),
  ];

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-live="polite"
        aria-label={t("pcCleanFastRun.runningTitle")}
        className="w-full max-w-md rounded-2xl border border-line bg-surface p-7"
      >
        {run.phase === "running" && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t("pcCleanFastRun.runningTitle")}</h2>
            <p className="mt-1 text-sm text-fg-3">{t("pcCleanFastRun.runningSub")}</p>
            <ul className="mt-5 grid gap-2.5">
              {steps.map((step) => (
                <li key={step} className="flex items-center gap-2.5 text-sm text-fg-2">
                  <span className="size-4 shrink-0 animate-spin rounded-full border-2 border-line border-t-accent" aria-hidden />
                  {step}
                </li>
              ))}
            </ul>
            <div className="mt-5 h-1.5 overflow-hidden rounded-full bg-surface-2">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-accent" />
            </div>
            <p className="mt-2 text-xs text-fg-3">
              {t("pcCleanFastRun.runningNote")} · {t("pcCleanFastRun.elapsed", { seconds: Math.max(0, Math.round((now - run.startedAt) / 1000)) })}
            </p>
          </>
        )}

        {run.phase === "done" && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t("pcCleanFastRun.doneTitle")}</h2>
            <p className="mt-1 text-sm text-fg-3">{t("pcCleanFastRun.doneSub")}</p>
            {run.before !== null && run.after !== null ? (
              <p className="mt-4 text-[28px] font-semibold tracking-tight text-fg">
                {t("pcCleanFastRun.scoreLine", { before: run.before, after: run.after })}
              </p>
            ) : (
              <p className="mt-4 text-sm text-fg-2">{t("pcCleanFastRun.scoreUnavailable")}</p>
            )}
            <h3 className="mt-4 text-xs font-medium uppercase tracking-wide text-fg-3">{t("pcCleanFastRun.appliedHeading")}</h3>
            {run.steps.length > 0 ? (
              <ul className="mt-2 grid gap-1.5 text-sm text-fg-2">
                {run.steps.map((name, index) => (
                  <li key={`${name}-${index}`} className="flex items-center gap-2">
                    <Check className="size-4 shrink-0 text-ok" />
                    {t(`pcCleanFastRun.stepNames.${name}`) === `pcCleanFastRun.stepNames.${name}`
                      ? name
                      : t(`pcCleanFastRun.stepNames.${name}`)}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 text-sm text-fg-2">{t("pcCleanFastRun.nothingApplied")}</p>
            )}
            <div className="mt-6 flex items-center justify-end gap-4">
              <Button
                variant="link"
                onClick={() => {
                  const sessionId = run.sessionId;
                  closePcCleanFastRun();
                  onUndo(sessionId);
                }}
              >
                {t("pcCleanFastRun.undo")}
              </Button>
              <Button variant="primary" onClick={closePcCleanFastRun}>
                {t("pcCleanFastRun.close")}
              </Button>
            </div>
          </>
        )}

        {run.phase === "failed" && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t("pcCleanFastRun.failedTitle")}</h2>
            <p className="mt-2 text-sm text-fg-2">{run.message}</p>
            <p className="mt-2 text-xs text-fg-3">{t("pcCleanFastRun.failedSub")}</p>
            <div className="mt-6 flex justify-end">
              <Button variant="primary" onClick={closePcCleanFastRun}>
                {t("pcCleanFastRun.close")}
              </Button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
