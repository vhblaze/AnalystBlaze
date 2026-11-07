import { useEffect, useLayoutEffect, useRef, useState, type ComponentType } from "react";
import {
  Activity,
  ArrowRight,
  Bell,
  Cpu,
  Gamepad2,
  Gauge,
  History,
  Lightbulb,
  ListChecks,
  PartyPopper,
  Search,
  Settings,
  SlidersHorizontal,
  Sparkles,
  ToggleRight,
  TrendingUp,
  Undo2,
  UserRound,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import type { Route } from "@/lib/routes";
import {
  TUTORIAL_STEPS,
  finishTutorial,
  placeCallout,
  tutorialBack,
  tutorialNext,
  useTutorial,
  type Box,
  type Placement,
} from "@/features/profile/tutorial";

const ICONS: Record<string, ComponentType<{ className?: string }>> = {
  how: ListChecks,
  search: Search,
  bell: Bell,
  weighing: Gauge,
  gameMode: Gamepad2,
  moreActions: Sparkles,
  pcStrip: Activity,
  lastOpt: TrendingUp,
  doneToday: Undo2,
  profile: UserRound,
  mypc: Cpu,
  insights: Lightbulb,
  controls: SlidersHorizontal,
  automations: ToggleRight,
  history: History,
  settings: Settings,
  done: PartyPopper,
};

const CARD_W = 380;
const PAD = 8;

function readBox(target: string): Box | null {
  const element = document.querySelector<HTMLElement>(`[data-tour="${target}"]`);
  if (!element) return null;
  const rect = element.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) return null;
  return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height };
}

function sameBox(a: Box | null, b: Box | null) {
  if (a === b) return true;
  if (!a || !b) return false;
  return Math.abs(a.left - b.left) < 0.5 && Math.abs(a.top - b.top) < 0.5 && Math.abs(a.width - b.width) < 0.5 && Math.abs(a.height - b.height) < 0.5;
}

/**
 * Tour guiado: abre a tela do passo, escurece o resto e destaca a parte
 * (data-tour) com um balão que diz o que ela faz e como funciona. Sem a parte
 * na tela (lista vazia, ainda carregando), o balão fica no centro. Esc pula,
 * setas navegam. O fundo bloqueia cliques para ninguém sair do tour sem querer.
 */
export function Tutorial({ navigate }: { navigate: (next: Route) => void }) {
  const { t } = useI18n();
  const { open, step } = useTutorial();
  const current = TUTORIAL_STEPS[step];
  const [box, setBox] = useState<Box | null>(null);
  const [size, setSize] = useState({ w: CARD_W, h: 320 });
  const [viewport, setViewport] = useState({ w: window.innerWidth, h: window.innerHeight });
  const cardRef = useRef<HTMLDivElement | null>(null);
  const scrolledFor = useRef<string | null>(null);

  // Abre a tela do passo.
  useEffect(() => {
    if (open && current?.route) navigate(current.route);
  }, [open, step, current, navigate]);

  // Acompanha o elemento (rola a tela, layout que muda, carregamento tardio).
  useEffect(() => {
    if (!open || !current?.target) {
      setBox(null);
      return;
    }
    const target = current.target;
    scrolledFor.current = null;
    let frame = 0;
    const loop = () => {
      const next = readBox(target);
      if (next && scrolledFor.current !== target) {
        scrolledFor.current = target;
        document.querySelector<HTMLElement>(`[data-tour="${target}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" });
      }
      setBox((previous) => (sameBox(previous, next) ? previous : next));
      setViewport((previous) =>
        previous.w === window.innerWidth && previous.h === window.innerHeight ? previous : { w: window.innerWidth, h: window.innerHeight },
      );
      frame = window.requestAnimationFrame(loop);
    };
    frame = window.requestAnimationFrame(loop);
    return () => window.cancelAnimationFrame(frame);
  }, [open, current]);

  // Altura real do balão, para posicionar sem cortar.
  useLayoutEffect(() => {
    const card = cardRef.current;
    if (!card) return;
    const next = { w: card.offsetWidth, h: card.offsetHeight };
    setSize((previous) => (previous.w === next.w && previous.h === next.h ? previous : next));
  });

  // Teclado.
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowRight") tutorialNext();
      else if (event.key === "ArrowLeft") tutorialBack();
      else if (event.key === "Escape") finishTutorial();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open || !current) return null;

  const total = TUTORIAL_STEPS.length;
  const last = step === total - 1;
  const Icon = ICONS[current.key] ?? Sparkles;
  const anchored = Boolean(current.target && box);
  const placement: Placement | null = anchored && box ? placeCallout(box, size, viewport) : null;
  const centered = !placement;
  const how = [1, 2, 3].map((index) => t(`tutorial.steps.${current.key}.h${index}`)).filter((text) => !text.startsWith("tutorial.steps."));
  const flow = current.kind === "intro" ? [t("tutorial.flow.n1"), t("tutorial.flow.n2"), t("tutorial.flow.n3"), t("tutorial.flow.n4")] : [];

  const cardStyle = centered
    ? { left: "50%", top: "50%", transform: "translate(-50%, -50%)" }
    : { left: placement.left, top: placement.top };

  return (
    <div className="fixed inset-0 z-[70]" role="dialog" aria-modal="true" aria-label={t("tutorial.title")}>
      {/* Bloqueia cliques no app; a luz do destaque abre um "buraco" nesta sombra. */}
      <div className={centered ? "absolute inset-0 bg-bg/80" : "absolute inset-0"} />
      {box && anchored && (
        <div
          className="pointer-events-none absolute rounded-[16px] border-2 border-accent transition-all duration-300 ease-out"
          style={{
            left: box.left - PAD,
            top: box.top - PAD,
            width: box.width + PAD * 2,
            height: box.height + PAD * 2,
            boxShadow: "0 0 0 9999px rgba(3,6,12,0.78), 0 0 28px rgba(34,211,238,0.35)",
          }}
        />
      )}

      <div
        ref={cardRef}
        className="absolute rounded-2xl border border-accent/40 bg-surface p-5 shadow-2xl transition-[left,top] duration-300 ease-out"
        style={{ width: CARD_W, maxWidth: "calc(100vw - 24px)", ...cardStyle }}
      >
        {placement && placement.side !== "none" && (
          <span
            aria-hidden
            className="absolute size-3 rotate-45 border-accent/40 bg-surface"
            style={
              placement.side === "right"
                ? { left: -7, top: placement.arrow - 6, borderLeftWidth: 1, borderBottomWidth: 1 }
                : placement.side === "left"
                  ? { right: -7, top: placement.arrow - 6, borderRightWidth: 1, borderTopWidth: 1 }
                  : placement.side === "bottom"
                    ? { top: -7, left: placement.arrow - 6, borderLeftWidth: 1, borderTopWidth: 1 }
                    : { bottom: -7, left: placement.arrow - 6, borderRightWidth: 1, borderBottomWidth: 1 }
            }
          />
        )}

        <div className="flex items-center justify-between text-xs text-fg-3">
          <span className="font-medium uppercase tracking-wide">
            {t("tutorial.title")} · {step + 1}/{total}
          </span>
          {!last && (
            <button type="button" onClick={finishTutorial} className="hover:text-fg">
              {t("tutorial.skip")}
            </button>
          )}
        </div>

        <div className="mt-3 flex items-start gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-accent/10 text-accent">
            <Icon className="size-5" />
          </span>
          <div className="min-w-0">
            <h3 className="text-[17px] font-semibold leading-snug text-fg">{t(`tutorial.steps.${current.key}.title`)}</h3>
            <p className="mt-1 text-sm leading-relaxed text-fg-2">{t(`tutorial.steps.${current.key}.body`)}</p>
          </div>
        </div>

        {flow.length > 0 && (
          <div className="mt-4 flex items-center justify-between gap-1">
            {flow.map((label, index) => (
              <div key={label} className="flex items-center gap-1">
                <span className="rounded-xl border border-line bg-surface-2 px-2.5 py-2 text-center text-xs font-medium text-fg-2">{label}</span>
                {index < flow.length - 1 && <ArrowRight className="size-3.5 text-accent" />}
              </div>
            ))}
          </div>
        )}

        {how.length > 0 && (
          <div className="mt-4">
            {!flow.length && current.kind !== "outro" && (
              <p className="mb-2 text-[11px] font-medium uppercase tracking-wide text-fg-3">{t("tutorial.howLabel")}</p>
            )}
            <ol className="grid gap-2">
              {how.map((text, index) => (
                <li key={text} className="flex items-start gap-2.5 text-[13px] leading-snug text-fg-2">
                  <span className="mt-px grid size-5 shrink-0 place-items-center rounded-full bg-surface-2 text-[11px] font-semibold text-accent">
                    {index + 1}
                  </span>
                  {text}
                </li>
              ))}
            </ol>
          </div>
        )}

        <div className="mt-5 flex items-center justify-between">
          <div className="flex gap-1" aria-hidden>
            {TUTORIAL_STEPS.map((item, index) => (
              <span key={item.key} className={"h-1 rounded-full transition-all " + (index === step ? "w-4 bg-accent" : index < step ? "w-1.5 bg-accent/50" : "w-1.5 bg-line")} />
            ))}
          </div>
          <div className="flex gap-2">
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
        <p className="mt-2 text-right text-[11px] text-fg-4">{t("tutorial.keys")}</p>
      </div>
    </div>
  );
}

