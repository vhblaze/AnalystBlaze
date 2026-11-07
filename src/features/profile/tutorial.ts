import { useSyncExternalStore } from "react";
import type { Route } from "@/lib/routes";
import { route } from "@/lib/routes";

/**
 * Tour guiado (primeiro contato): cada passo abre a tela, destaca uma parte
 * dela (elemento com `data-tour="<target>"`) e explica o que a funcionalidade
 * faz e como funciona. Estado de módulo: o AppShell desenha, e
 * Configurações › "Rever tutorial" chama `startTutorial()` sem prop.
 *
 * `target: null` = balão no centro (abertura e encerramento). Se o elemento
 * não existir na tela (ex.: lista vazia), o balão também cai no centro.
 */
export type TourStep = {
  key: string;
  route: Route | null;
  target: string | null;
  kind?: "intro" | "outro";
};

export const TUTORIAL_STEPS: ReadonlyArray<TourStep> = [
  { key: "how", route: null, target: null, kind: "intro" },
  { key: "search", route: route("dashboard"), target: "search" },
  { key: "bell", route: route("dashboard"), target: "bell" },
  { key: "weighing", route: route("dashboard"), target: "weighing" },
  { key: "gameMode", route: route("dashboard"), target: "game-mode" },
  { key: "moreActions", route: route("dashboard"), target: "more-actions" },
  { key: "pcStrip", route: route("dashboard"), target: "pc-strip" },
  { key: "lastOpt", route: route("dashboard"), target: "last-opt" },
  { key: "doneToday", route: route("dashboard"), target: "done-today" },
  { key: "profile", route: route("dashboard"), target: "usage-profile" },
  { key: "mypc", route: route("mypc", "performance"), target: "mypc-tabs" },
  { key: "insights", route: route("insights"), target: "insights-list" },
  { key: "controls", route: route("controls"), target: "controls-list" },
  { key: "automations", route: route("controls"), target: "automations" },
  { key: "history", route: route("history"), target: "history-list" },
  { key: "settings", route: route("settings"), target: "settings-profile" },
  { key: "done", route: null, target: null, kind: "outro" },
];

export const FIRST_CONTACT_DONE = "analystblaze.firstContact.done.v1";
export const TUTORIAL_DONE = "analystblaze.tutorial.done.v1";

type State = { open: boolean; step: number };
let state: State = { open: false, step: 0 };
const listeners = new Set<() => void>();

function set(next: State) {
  state = next;
  listeners.forEach((listener) => listener());
}

export function useTutorial(): State {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => state,
  );
}

export function startTutorial() {
  set({ open: true, step: 0 });
}

export function tutorialNext() {
  if (state.step + 1 >= TUTORIAL_STEPS.length) return finishTutorial();
  set({ open: true, step: state.step + 1 });
}

export function tutorialBack() {
  set({ open: true, step: Math.max(0, state.step - 1) });
}

export function finishTutorial() {
  writeFlag(TUTORIAL_DONE);
  set({ open: false, step: 0 });
}

export function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

export function writeFlag(key: string) {
  try {
    localStorage.setItem(key, "1");
  } catch {
    // Best effort.
  }
}

// ---------------------------------------------------------------------------
// Posição do balão (pura: o componente só mede e aplica).

export type Side = "right" | "left" | "bottom" | "top" | "none";
export type Box = { left: number; top: number; right: number; bottom: number; width: number; height: number };

export type Placement = {
  left: number;
  top: number;
  side: Side;
  /** Deslocamento da seta ao longo da borda do balão (px). */
  arrow: number;
};

const EDGE = 12;
const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), Math.max(min, max));

/**
 * Escolhe de que lado do elemento o balão cabe, na ordem que combina com a
 * forma dele (largo → embaixo/em cima; alto/estreito → direita/esquerda).
 * Se nada cabe (elemento maior que a tela), o balão vai para o canto.
 */
export function placeCallout(box: Box, card: { w: number; h: number }, viewport: { w: number; h: number }, gap = 18): Placement {
  const wide = box.width > viewport.w * 0.55;
  const order: Side[] = wide ? ["bottom", "top", "right", "left"] : ["right", "left", "bottom", "top"];
  const room: Record<Exclude<Side, "none">, number> = {
    right: viewport.w - box.right - EDGE,
    left: box.left - EDGE,
    bottom: viewport.h - box.bottom - EDGE,
    top: box.top - EDGE,
  };
  const need = { right: card.w + gap, left: card.w + gap, bottom: card.h + gap, top: card.h + gap };
  const centerX = box.left + box.width / 2;
  const centerY = box.top + box.height / 2;

  for (const side of order) {
    if (side === "none" || room[side] < need[side]) continue;
    if (side === "right" || side === "left") {
      const top = clamp(centerY - card.h / 2, EDGE, viewport.h - card.h - EDGE);
      const left = side === "right" ? box.right + gap : box.left - gap - card.w;
      return { left, top, side, arrow: clamp(centerY - top, 24, card.h - 24) };
    }
    const left = clamp(centerX - card.w / 2, EDGE, viewport.w - card.w - EDGE);
    const top = side === "bottom" ? box.bottom + gap : box.top - gap - card.h;
    return { left, top, side, arrow: clamp(centerX - left, 24, card.w - 24) };
  }
  return { left: viewport.w - card.w - 24, top: viewport.h - card.h - 24, side: "none", arrow: 0 };
}
