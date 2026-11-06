import { useSyncExternalStore } from "react";
import type { Route } from "@/lib/routes";
import { route } from "@/lib/routes";

/**
 * Tutorial completo (primeiro contato): um passo por tela, cada um abre a
 * tela e explica o que ela faz. Estado de módulo: o AppShell desenha, e
 * Configurações › "Rever tutorial" chama `startTutorial()` sem prop.
 */

export const TUTORIAL_STEPS: ReadonlyArray<{ key: string; route: Route | null }> = [
  { key: "how", route: null },
  { key: "dashboard", route: route("dashboard") },
  { key: "mypc", route: route("mypc", "performance") },
  { key: "insights", route: route("insights") },
  { key: "controls", route: route("controls") },
  { key: "history", route: route("history") },
  { key: "settings", route: route("settings") },
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
