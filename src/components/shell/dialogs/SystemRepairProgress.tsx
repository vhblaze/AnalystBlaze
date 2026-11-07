import { Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";
import { closeRepairPanel, useRepairPanel, useSystemRepair, type RepairKind } from "@/features/health/useSystemRepair";

/**
 * Pop-up da verificação do Windows (SFC) e do reparo DISM, no mesmo estilo do
 * "PC limpo e rápido": abre depois que a pessoa confirma, mostra o que está
 * sendo feito e o tempo, e no fim o resultado. O Windows não informa
 * percentual (system_repair.rs), então não há porcentagem inventada: a barra
 * é contínua e o que cresce é o tempo decorrido.
 */
export function SystemRepairProgress({ onStartDism }: { onStartDism: () => Promise<unknown> }) {
  const { t } = useI18n();
  const kind = useRepairPanel();
  const repair = useSystemRepair();
  if (!kind) return null;

  const state = repair[kind];
  const status = state.status;
  const running = Boolean(state.scanId) || state.starting;
  const done = Boolean(status?.done && status.result);
  const outcome = status?.result?.outcome;
  const tone = status?.result?.success ? "ok" : outcome === "needs_dism_repair" ? "warn" : "danger";
  const steps = [1, 2, 3].map((index) => t(`repairPopup.${kind}.step${index}`));
  const needsDism = kind === "sfc" && outcome === "needs_dism_repair";

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-live="polite"
        aria-label={t(`repairPopup.${kind}.runningTitle`)}
        className="w-full max-w-md rounded-2xl border border-line bg-surface p-7"
      >
        {running && !done && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t(`repairPopup.${kind}.runningTitle`)}</h2>
            <p className="mt-1 text-sm text-fg-3">{t(`repairPopup.${kind}.sub`)}</p>
            <ul className="mt-5 grid gap-2.5">
              {steps.map((step) => (
                <li key={step} className="flex items-start gap-2.5 text-sm text-fg-2">
                  <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-accent" aria-hidden />
                  {step}
                </li>
              ))}
            </ul>
            <div className="mt-5 h-1.5 overflow-hidden rounded-full bg-surface-2">
              <div className="h-full w-1/3 animate-pulse rounded-full bg-accent" />
            </div>
            <p className="mt-2 text-xs text-fg-3">
              {state.starting
                ? t("controls.d3.repair.starting")
                : t("repairPopup.elapsed", { elapsed: formatDuration(status?.elapsedSeconds ?? 0) })}
            </p>
            <p className="mt-3 text-xs text-fg-3">{t(`repairPopup.${kind}.note`)}</p>
            <div className="mt-5 flex justify-end">
              <Button variant="outline" onClick={closeRepairPanel}>
                {t("repairPopup.background")}
              </Button>
            </div>
          </>
        )}

        {done && status?.result && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t("repairPopup.doneTitle")}</h2>
            <p className={cn("mt-3 flex items-start gap-2 text-sm font-medium", tone === "ok" ? "text-ok" : tone === "warn" ? "text-warn" : "text-danger")}>
              {tone === "ok" && <Check className="mt-0.5 size-4 shrink-0" />}
              {outcome && t(`controls.d3.repair.outcome.${outcome}`) !== `controls.d3.repair.outcome.${outcome}`
                ? t(`controls.d3.repair.outcome.${outcome}`)
                : t("controls.d3.repair.outcome.unknown")}
            </p>
            {outcome === "unknown" && status.result.rawTail && (
              <pre className="mt-3 max-h-32 overflow-auto rounded-control border border-line bg-bg/60 p-2 text-[11px] text-fg-3">
                {status.result.rawTail}
              </pre>
            )}
            <p className="mt-3 text-xs text-fg-3">{t("repairPopup.counterNote")}</p>
            <div className="mt-6 flex items-center justify-end gap-3">
              {needsDism ? (
                <>
                  <Button variant="outline" onClick={closeRepairPanel}>
                    {t("repairPopup.close")}
                  </Button>
                  <Button
                    variant="primary"
                    onClick={() => {
                      closeRepairPanel();
                      void onStartDism();
                    }}
                  >
                    {t("repairPopup.runDism")}
                  </Button>
                </>
              ) : (
                <Button variant="primary" onClick={closeRepairPanel}>
                  {t("repairPopup.close")}
                </Button>
              )}
            </div>
          </>
        )}

        {!running && !done && (
          <>
            <h2 className="text-xl font-semibold text-fg">{t("repairPopup.failedTitle")}</h2>
            <p className="mt-2 text-sm text-fg-2">{state.error ?? t("controls.d3.repair.outcome.failed")}</p>
            <div className="mt-6 flex justify-end">
              <Button variant="primary" onClick={closeRepairPanel}>
                {t("repairPopup.close")}
              </Button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}

export type { RepairKind };
