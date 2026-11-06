import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import type { Route } from "@/lib/routes";
import { TUTORIAL_STEPS, finishTutorial, tutorialBack, tutorialNext, useTutorial } from "@/features/profile/tutorial";

/**
 * Tutorial completo: um painel fixo no canto inferior direito que abre a
 * tela de cada passo por trás e explica o que ela faz. Não bloqueia a tela
 * (o usuário pode clicar no que está vendo). "Pular" fecha de vez.
 */
export function Tutorial({ navigate }: { navigate: (next: Route) => void }) {
  const { t } = useI18n();
  const { open, step } = useTutorial();
  const current = TUTORIAL_STEPS[step];

  useEffect(() => {
    if (open && current?.route) navigate(current.route);
  }, [open, step, current, navigate]);

  if (!open || !current) return null;
  const last = step === TUTORIAL_STEPS.length - 1;
  const bullets = [1, 2, 3].map((index) => t(`tutorial.steps.${current.key}.b${index}`));

  return (
    <aside
      role="dialog"
      aria-label={t("tutorial.title")}
      className="fixed bottom-6 right-6 z-40 w-[420px] rounded-2xl border border-accent/40 bg-surface p-6 shadow-2xl"
    >
      <div className="flex items-center justify-between">
        <p className="text-xs font-medium uppercase tracking-wide text-fg-3">
          {t("tutorial.title")} · {step + 1}/{TUTORIAL_STEPS.length}
        </p>
        <button type="button" onClick={finishTutorial} className="text-xs text-fg-3 hover:text-fg">
          {t("tutorial.skip")}
        </button>
      </div>
      <h3 className="mt-2 text-lg font-semibold text-fg">{t(`tutorial.steps.${current.key}.title`)}</h3>
      <p className="mt-1 text-sm leading-relaxed text-fg-2">{t(`tutorial.steps.${current.key}.body`)}</p>
      {bullets.length > 0 && (
        <ul className="mt-3 grid gap-1 text-sm text-fg-2">
          {bullets.map((text) => (
            <li key={text} className="flex gap-2">
              <span className="text-accent">•</span>
              {text}
            </li>
          ))}
        </ul>
      )}
      <div className="mt-5 flex items-center justify-between">
        <div className="flex gap-1.5">
          {TUTORIAL_STEPS.map((item, index) => (
            <span key={item.key} className={"size-1.5 rounded-full " + (index === step ? "bg-accent" : "bg-line")} />
          ))}
        </div>
        <div className="flex gap-3">
          {step > 0 && (
            <Button size="sm" variant="outline" onClick={tutorialBack}>
              {t("tutorial.back")}
            </Button>
          )}
          <Button size="sm" variant="primary" onClick={tutorialNext}>
            {last ? t("tutorial.finish") : t("tutorial.next")}
          </Button>
        </div>
      </div>
    </aside>
  );
}
