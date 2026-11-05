/**
 * Ações que Controles (e Meu PC › Saúde) disparam. Cada uma já pede a
 * confirmação local antes de rodar e devolve `false` quando a pessoa cancela
 * (montadas no ScreenOutlet com runConfirmed).
 */
export type PowerPlan = "high_performance" | "balanced" | "power_saver";

export type ControlsActions = {
  onActivateGameMode: () => Promise<unknown>;
  onRestoreGameMode: () => Promise<unknown>;
  onRestoreOptimizations: () => Promise<unknown>;
  onApplyPcCleanFast: () => Promise<unknown>;
  onRestorePerformanceSession: (sessionId?: string | null) => Promise<unknown>;
  onStartSystemFileCheck: () => Promise<unknown>;
  onStartDismRestoreHealth: () => Promise<unknown>;
  onDisableStartup: (name: string, location?: string | null) => Promise<unknown>;
  onRestoreStartup: (name?: string | null) => Promise<unknown>;
  onDelayStartupApp: (name: string, location?: string | null) => Promise<unknown>;
  onRestoreDelayedStartupApp: (name?: string | null) => Promise<unknown>;
  onStopService: (name: string) => Promise<unknown>;
  onRestoreService: (name?: string | null) => Promise<unknown>;
  onSetPowerPlan: (plan: PowerPlan) => Promise<unknown>;
  onApplyVisualPerformance: () => Promise<unknown>;
  onRestoreVisualPerformance: () => Promise<unknown>;
  onDeepCleanTemp: () => Promise<unknown>;
  onPurgeCleanup: () => Promise<unknown>;
  onApplyCleanupCategory: (category: string, mode?: string | null) => Promise<unknown>;
  onDisableGameDvr: () => Promise<unknown>;
};

/** `false` = a pessoa cancelou a confirmação. */
export function cancelled(result: unknown): boolean {
  return result === false;
}
