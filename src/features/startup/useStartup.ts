import { useMemo } from "react";
import {
  getDelayedStartupApps,
  scanStartupImpact,
  type DelayedStartupApp,
  type OptimizationSnapshot,
  type StartupImpact,
} from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";
import { optimizationSnapshots } from "@/features/history/useHistory";

/** Apps que abrem com o Windows e o impacto de cada um. Fonte única (doc 14 Q1). */
export const startupImpact = createAgentResource<StartupImpact[]>({
  name: "startup.impact",
  fetch: scanStartupImpact,
});

/** Apps tirados da inicialização pelo "Atrasar" (que hoje não reabre o app sozinho). */
export const delayedStartupApps = createAgentResource<DelayedStartupApp[]>({
  name: "startup.delayed",
  fetch: getDelayedStartupApps,
});

/** Aberto há menos que isso = em uso: nunca sugerimos tirar. */
const RECENT_USE_DAYS = 7;

export type ImpactLevel = "high" | "medium" | "low";

/** Mesmos cortes que o agente usa para recomendar (performance_suite.rs). */
export function impactLevel(score: number): ImpactLevel {
  if (score >= 40) return "high";
  if (score >= 32) return "medium";
  return "low";
}

export type StartupRow = {
  name: string;
  location: string | null;
  publisher: string | null;
  impact: ImpactLevel | null;
  /** Dias desde a última vez que você abriu; null = o agente não sabe. */
  lastSeenDaysAgo: number | null;
  /** "on": abre com o Windows. "off": o AnalystBlaze tirou (pode voltar). */
  state: "on" | "off";
  /** Como voltar: tirado pelo "Atrasar" ou pelo "Desativar". */
  offVia: "delay" | "disable" | null;
  /** Só apps marcados como seguros pelo agente podem ser tirados. */
  canChange: boolean;
  /** O agente recomenda tirar (inclusive por "nunca aberto"). */
  suggested: boolean;
};

function disabledBySnapshots(snapshots: OptimizationSnapshot[]): Map<string, string> {
  const names = new Map<string, string>();
  for (const snapshot of snapshots) {
    if (snapshot.restored_at || snapshot.action_name !== "DISABLE_STARTUP_APP") continue;
    for (const entry of snapshot.entries) {
      const e = entry as Record<string, unknown> | null;
      if (e?.kind === "startup_registry_value" && typeof e.value_name === "string") {
        names.set(e.value_name.toLowerCase(), e.value_name);
      }
    }
  }
  return names;
}

/** Uma linha por app: os que abrem com o Windows e os que o AnalystBlaze tirou. */
export function buildStartupRows(
  apps: StartupImpact[],
  delayed: DelayedStartupApp[],
  snapshots: OptimizationSnapshot[],
): StartupRow[] {
  const rows: StartupRow[] = apps.map((app) => ({
    name: app.name,
    location: app.location,
    publisher: app.publisher ?? null,
    impact: impactLevel(app.impactScore),
    lastSeenDaysAgo: app.lastSeenDaysAgo ?? null,
    state: "on",
    offVia: null,
    canChange: app.availableActions.includes("disable"),
    // O agente recomenda e você não abriu o app nos últimos 7 dias (ou ainda
    // não há dado de uso). Sem isso, um app usado hoje apareceria como
    // "sugerido tirar" só pelo nome.
    suggested:
      (app.recommendation === "disable" || app.recommendation === "delay") &&
      (app.lastSeenDaysAgo == null || app.lastSeenDaysAgo >= RECENT_USE_DAYS),
  }));
  const seen = new Set(rows.map((row) => row.name.toLowerCase()));
  for (const app of delayed) {
    if (seen.has(app.name.toLowerCase())) continue;
    seen.add(app.name.toLowerCase());
    rows.push({ name: app.name, location: app.location, publisher: null, impact: null, lastSeenDaysAgo: null, state: "off", offVia: "delay", canChange: true, suggested: false });
  }
  for (const [key, name] of disabledBySnapshots(snapshots)) {
    if (seen.has(key)) continue;
    seen.add(key);
    rows.push({ name, location: null, publisher: null, impact: null, lastSeenDaysAgo: null, state: "off", offVia: "disable", canChange: true, suggested: false });
  }
  return rows;
}

export function useStartup(options?: { auto?: boolean }) {
  const resource = startupImpact.use(options);
  const delayed = delayedStartupApps.use(options);
  const snapshots = optimizationSnapshots.use(options);
  const rows = useMemo(
    () => buildStartupRows(resource.data ?? [], delayed.data ?? [], snapshots.data ?? []),
    [resource.data, delayed.data, snapshots.data],
  );
  return {
    apps: resource.data ?? [],
    rows,
    loading: resource.loading || delayed.loading,
    error: resource.error,
    refresh: () => Promise.allSettled([resource.refresh(), delayed.refresh(), snapshots.refresh()]),
    set: resource.set,
  };
}
