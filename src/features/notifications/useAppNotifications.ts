import { useEffect, useRef, useState } from "react";
import type { Locale } from "@/i18n";
import { displayGameName, formatMegabytes, formatSignedFps } from "@/lib/format";
import type { AuditEvent, LocalAiPolicy } from "@/services/tauri/agent";
import { auditLog, todaysAutomaticActions } from "@/features/history/useHistory";
import { markInsightsSeen, serverInsights, unseenInsights } from "@/features/insights/useInsights";
import { useAutomationPolicy } from "@/features/automations/useAutomationPolicy";

/**
 * Avisos do Windows (doc 11 / board Tela-Configuracoes), três gatilhos,
 * cada um com o próprio interruptor em Configurações › Notificações:
 * - "quando algo melhorar de verdade": uma ação automática com resultado
 *   medido (1% low subiu, ou liberou pelo menos 512 MB);
 * - "quando uma sugestão nova aparecer";
 * - "resumo do dia às 22:00", só se o app estiver aberto nesse horário.
 * Nunca abre janela: é a notificação do Windows, e só com permissão dada.
 */

const AUDIT_POLL_MS = 60_000;
const INSIGHTS_POLL_MS = 30 * 60_000;
const CLOCK_POLL_MS = 60_000;
const FREED_MIN_MB = 512;
const LAST_AUDIT_STORAGE = "analystblaze.notify.lastAudit.v1";
const LAST_DAILY_STORAGE = "analystblaze.notify.lastDaily.v1";
const DAILY_HOUR = 22;

type Translate = (key: string, params?: Record<string, string | number>) => string;

function canNotify(): boolean {
  return typeof Notification !== "undefined" && Notification.permission === "granted";
}

function notify(title: string, body: string) {
  if (!canNotify()) return;
  try {
    new Notification(title, { body, silent: false });
  } catch {
    // O Windows pode recusar; nunca é erro para o usuário.
  }
}

/** Pede a permissão do Windows uma vez (ao ligar um interruptor). */
export async function ensureNotificationPermission(): Promise<NotificationPermission | "unsupported"> {
  if (typeof Notification === "undefined") return "unsupported";
  if (Notification.permission !== "default") return Notification.permission;
  try {
    return await Notification.requestPermission();
  } catch {
    return "denied";
  }
}

type ImprovementDetails = {
  action_name?: string;
  source?: string;
  success?: boolean;
  details?: { freed_mb?: unknown; frameMeasurement?: { verdict?: string; deltaLow1PctFps?: number | null } | null; detected_game?: { process_name?: string | null } | null };
};

/** Texto do aviso para uma ação automática que melhorou algo medido, ou null. */
export function improvementMessage(event: AuditEvent, t: Translate): string | null {
  const d = (event.details && typeof event.details === "object" ? event.details : {}) as ImprovementDetails;
  if (!d.success || d.source !== "local_policy") return null;
  const fps = d.details?.frameMeasurement;
  if (fps?.verdict === "better" && typeof fps.deltaLow1PctFps === "number") {
    const game = d.details?.detected_game?.process_name;
    return game
      ? t("notify.improvedFpsGame", { delta: formatSignedFps(fps.deltaLow1PctFps), game: displayGameName(game) })
      : t("notify.improvedFps", { delta: formatSignedFps(fps.deltaLow1PctFps) });
  }
  const freed = Number(d.details?.freed_mb);
  if (Number.isFinite(freed) && freed >= FREED_MIN_MB) return t("notify.freed", { size: formatMegabytes(freed) });
  return null;
}

function readNumber(key: string): number {
  try {
    return Number(localStorage.getItem(key)) || 0;
  } catch {
    return 0;
  }
}

function writeValue(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Best effort.
  }
}

export function useAppNotifications({
  enabled,
  locale,
  t,
  onInsightsScreen,
}: {
  /** Agente conectado: sem ele não há ações nem sugestões. */
  enabled: boolean;
  locale: Locale;
  t: Translate;
  /** A tela de Insights está aberta: tudo o que chegar conta como visto. */
  onInsightsScreen: boolean;
}) {
  const { policy } = useAutomationPolicy({ auto: enabled });
  const policyRef = useRef<LocalAiPolicy | null>(null);
  policyRef.current = policy;
  const [unseenCount, setUnseenCount] = useState(0);
  const insightsRead = serverInsights.use({ auto: false });

  // 1) Melhorou de verdade: ações automáticas com resultado medido.
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const check = async () => {
      if (!policyRef.current?.notify_on_improvement) return;
      const events = await auditLog.refresh().catch(() => null);
      if (disposed || !events) return;
      const since = readNumber(LAST_AUDIT_STORAGE) || Math.floor(Date.now() / 1000);
      let newest = since;
      for (const event of events) {
        if (event.timestamp <= since) continue;
        newest = Math.max(newest, event.timestamp);
        const message = improvementMessage(event, t);
        if (message) notify(t("notify.improvedTitle"), message);
      }
      writeValue(LAST_AUDIT_STORAGE, String(newest));
    };
    void check();
    const id = window.setInterval(() => void check(), AUDIT_POLL_MS);
    return () => {
      disposed = true;
      window.clearInterval(id);
    };
  }, [enabled, t]);

  // 2) Sugestão nova + ponto de novidade na sidebar.
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const check = async () => {
      const insights = await serverInsights.refresh(locale).catch(() => null);
      if (disposed || !insights) return;
      const fresh = unseenInsights(insights);
      setUnseenCount(fresh.length);
      if (fresh.length > 0 && policyRef.current?.notify_on_new_insight && !onInsightsScreen) {
        notify(t("notify.newInsightTitle"), fresh.length === 1 ? fresh[0].title : t("notify.newInsights", { count: fresh.length }));
        // Avisado uma vez: não repete a cada leitura.
        markInsightsSeen(fresh);
      }
    };
    void check();
    const id = window.setInterval(() => void check(), INSIGHTS_POLL_MS);
    return () => {
      disposed = true;
      window.clearInterval(id);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, locale]);

  // Abrir a tela de Insights zera o ponto.
  useEffect(() => {
    if (!onInsightsScreen) return;
    markInsightsSeen(insightsRead.data ?? []);
    setUnseenCount(0);
  }, [onInsightsScreen, insightsRead.data]);

  // 3) Resumo do dia às 22:00, se o app estiver aberto.
  useEffect(() => {
    if (!enabled) return;
    const tick = async () => {
      if (!policyRef.current?.notify_daily_summary) return;
      const now = new Date();
      if (now.getHours() !== DAILY_HOUR) return;
      const today = now.toISOString().slice(0, 10);
      try {
        if (localStorage.getItem(LAST_DAILY_STORAGE) === today) return;
      } catch {
        // Sem localStorage, avisa uma vez por sessão (o ref abaixo).
      }
      const actions = await todaysAutomaticActions.refresh().catch(() => []);
      writeValue(LAST_DAILY_STORAGE, today);
      notify(t("notify.dailyTitle"), actions.length > 0 ? t("notify.dailyBody", { count: actions.length }) : t("notify.dailyNone"));
    };
    const id = window.setInterval(() => void tick(), CLOCK_POLL_MS);
    return () => window.clearInterval(id);
  }, [enabled, t]);

  return { unseenInsights: unseenCount };
}
