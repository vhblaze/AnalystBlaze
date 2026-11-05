import type { AuditEvent, OptimizationSnapshot } from "@/services/tauri/agent";
import { formatBytes, formatMegabytes } from "@/lib/format";

/**
 * Linha do tempo do Histórico e do "O que foi feito hoje" (doc 13 §4).
 * Junta snapshots (o que pode voltar atrás) e a auditoria (quem disparou e
 * o que foi medido). Só mostra o que o agente realmente guardou (doc 14 H3).
 */

/** Quem disparou: o agente sozinho, você no app ou você pelo site. null = a auditoria não diz. */
export type TimelineSource = "auto" | "you" | "web" | null;

export type TimelineChange = {
  label: string;
  before?: string | null;
  after?: string | null;
};

export type TimelineItem = {
  id: string;
  /** Unix em segundos. */
  at: number;
  /** Chave i18n do título, com o código da ação como reserva. */
  titleKey: string;
  actionName: string;
  source: TimelineSource;
  /** Snapshot que este item desfaz; sem snapshot não há Desfazer. */
  snapshotId: string | null;
  restoredAt: number | null;
  changes: TimelineChange[];
  /** Resultado medido pelo agente ("liberou 3,2 GB"), quando houver. */
  result: string | null;
};

const SOURCE_BY_AUDIT: Record<string, TimelineSource> = {
  local_policy: "auto",
  manual_user: "you",
  remote_command: "web",
};

/** Eventos que o agente registra quando age sozinho e que não passam por comando. */
const AUTOMATIC_EVENTS: Record<string, string> = {
  "shadow_storage.auto_resized": "history.actions.SHADOW_STORAGE_AUTO_RESIZED",
  "service_usage.auto_restored": "history.actions.SERVICE_AUTO_RESTORED",
  "quarantine.auto_pruned": "history.actions.QUARANTINE_AUTO_PRUNED",
  "game_mode.restored_after_game_exit": "history.actions.GAME_MODE_RESTORED_AFTER_EXIT",
  "focus.restored_after_ttl": "history.actions.FOCUS_RESTORED_AFTER_TTL",
  "network_tune.reverted_after_timeout": "history.actions.NETWORK_TUNE_REVERTED_TIMEOUT",
};

/** Comandos sem snapshot que mudam algo e por isso entram na linha do tempo. */
const COMMANDS_WITHOUT_SNAPSHOT = new Set([
  "CLEAR_STANDBY_LIST",
  "FLUSH_DNS_CACHE",
  "RENEW_DHCP_LEASE",
  "RESET_WINSOCK_CATALOG",
  "PURGE_CLEANUP_QUARANTINE",
  "DELETE_DISK_USAGE_ITEM",
  "ENABLE_SCHEDULED_DEFRAG",
  "RESTART_PNP_DEVICE",
  "RENEW_DEFENDER_DEFINITIONS",
  "SCHEDULE_VOLUME_CHECK",
]);

const KNOWN_ACTIONS = new Set([
  "APPLY_GAME_MODE",
  "APPLY_PC_CLEAN_FAST_PROFILE",
  "APPLY_PC_CLEAN_FAST_BACKGROUND_PRIORITIES",
  "APPLY_BACKGROUND_QUIET_MODE",
  "APPLY_FOREGROUND_BURST_MODE",
  "APPLY_ADAPTIVE_OPTIMIZATION",
  "APPLY_UPLINK_PRESSURE_RELIEF_STAGE1",
  "APPLY_LATENCY_TWEAKS",
  "APPLY_CLEANUP_CATEGORY",
  "APPLY_VISUAL_PERFORMANCE_MODE",
  "SET_PROCESS_PRIORITY",
  "EMPTY_TEMP",
  "PURGE_CLEANUP_QUARANTINE",
  "CLEAR_STANDBY_LIST",
  "FLUSH_DNS_CACHE",
  "RENEW_DHCP_LEASE",
  "APPLY_NETWORK_TUNE",
  "RESET_WINSOCK_CATALOG",
  "SET_DNS_SERVERS",
  "SET_INTERFACE_METRIC",
  "SET_ADAPTER_ENABLED",
  "SET_POWER_PLAN_HIGH_PERFORMANCE",
  "SET_POWER_PLAN_BALANCED",
  "SET_POWER_PLAN_POWER_SAVER",
  "ENTER_FOCUS_MODE",
  "DELAY_STARTUP_APP",
  "DISABLE_STARTUP_APP",
  "STOP_SERVICE",
  "DELETE_DISK_USAGE_ITEM",
  "ENABLE_SCHEDULED_DEFRAG",
  "RESTART_PNP_DEVICE",
  "DISABLE_GAME_DVR",
  "REPAIR_SERVICE",
  "DISABLE_SERVICE_PERMANENTLY",
  "THROTTLE_DEFENDER_SCANS",
  "RENEW_DEFENDER_DEFINITIONS",
  "SCHEDULE_VOLUME_CHECK",
]);

export function actionTitleKey(actionName: string): string {
  return KNOWN_ACTIONS.has(actionName) ? `history.actions.${actionName}` : "history.actions.UNKNOWN";
}

type AuditDetails = { action_name?: string; source?: string; success?: boolean; details?: Record<string, unknown> };

function auditDetails(event: AuditEvent): AuditDetails {
  return event.details && typeof event.details === "object" ? (event.details as AuditDetails) : {};
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function basename(path: unknown): string | null {
  const value = text(path);
  if (!value) return null;
  const parts = value.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? value;
}

/** "O que mudou" de cada entrada do snapshot, só com o que o agente guardou. */
export function describeEntry(entry: unknown, t: (key: string, params?: Record<string, string | number>) => string): TimelineChange | null {
  if (!entry || typeof entry !== "object") return null;
  const e = entry as Record<string, unknown>;
  switch (e.kind) {
    case "power_plan":
      return { label: t("history.change.powerPlan"), before: text(e.previous_scheme_name), after: text(e.target_scheme) };
    case "power_overlay_scheme":
      return { label: t("history.change.powerMode"), before: text(e.previous_overlay_scheme), after: text(e.target_overlay_scheme) };
    case "quarantined_path":
      return {
        label: basename(e.original_path) ?? t("history.change.file"),
        after: t("history.change.quarantined", { size: formatBytes(Number(e.bytes)) }),
      };
    case "removed_empty_dir":
      return { label: basename(e.original_path) ?? t("history.change.folder"), after: t("history.change.removedEmptyDir") };
    case "startup_registry_value":
      return { label: text(e.value_name) ?? t("history.change.startupApp"), before: t("history.change.startsWithWindows"), after: t("history.change.notAtStartup") };
    case "service_state":
      return {
        label: text(e.display_name) ?? text(e.service_name) ?? t("history.change.service"),
        before: e.was_running ? t("history.change.running") : t("history.change.stopped"),
        after: t("history.change.changed"),
      };
    case "process_priority":
      return { label: t("history.change.priorityOf", { name: text(e.process_name) ?? "?" }), before: text(e.previous_priority_label), after: text(e.target_priority_label) };
    case "process_efficiency":
      return { label: t("history.change.efficiencyOf", { name: text(e.process_name) ?? "?" }), after: t("history.change.changed") };
    case "process_affinity":
      return { label: t("history.change.affinityOf", { name: text(e.process_name) ?? "?" }), after: t("history.change.changed") };
    case "dns_configuration":
      return {
        label: t("history.change.dnsOf", { name: text(e.adapter_name) ?? "?" }),
        before: e.was_dhcp ? t("history.change.automatic") : Array.isArray(e.previous_dns_servers) ? e.previous_dns_servers.join(", ") : null,
        after: t("history.change.changed"),
      };
    case "interface_metric":
      return { label: t("history.change.metricOf", { name: text(e.adapter_name) ?? "?" }), before: e.previous_automatic ? t("history.change.automatic") : String(e.previous_metric ?? ""), after: t("history.change.changed") };
    case "adapter_enabled":
      return {
        label: text(e.adapter_name) ?? t("history.change.adapter"),
        before: e.previous_enabled ? t("history.change.enabled") : t("history.change.disabled"),
        after: e.previous_enabled ? t("history.change.disabled") : t("history.change.enabled"),
      };
    case "tcp_global_setting":
      return { label: text(e.property) ?? "TCP", before: text(e.previous_value) ?? t("history.change.default"), after: t("history.change.changed") };
    case "registry_value":
      return { label: text(e.value_name) ?? t("history.change.windowsSetting"), after: t("history.change.changed") };
    case "defender_scan_preference":
      return { label: text(e.property) ?? "Defender", before: text(e.previous_value), after: t("history.change.changed") };
    default:
      return null;
  }
}

function quarantinedBytes(snapshot: OptimizationSnapshot): number {
  return snapshot.entries.reduce<number>((sum, entry) => {
    const e = entry as Record<string, unknown> | null;
    return e && e.kind === "quarantined_path" ? sum + (Number(e.bytes) || 0) : sum;
  }, 0);
}

function measuredResult(event: AuditEvent | undefined, t: (key: string, params?: Record<string, string | number>) => string): string | null {
  if (!event) return null;
  const inner = auditDetails(event).details;
  if (!inner || typeof inner !== "object") return null;
  const record = inner as Record<string, unknown>;
  const freed = Number(record.freed_mb);
  if (Number.isFinite(freed) && freed > 0) return t("history.result.freedMemory", { size: formatMegabytes(freed) });
  // L4: antes/depois em 1% low guardado junto com a ação do Modo Jogo.
  const fps = record.frameMeasurement as { verdict?: string; baseline?: { low1PctFps?: number | null }; optimized?: { low1PctFps?: number | null }; noiseFps?: number } | null | undefined;
  const before = fps?.baseline?.low1PctFps;
  const after = fps?.optimized?.low1PctFps;
  if (fps?.verdict && typeof before === "number" && typeof after === "number") {
    if (fps.verdict === "no_measurable_difference") return t("history.result.fpsSame", { noise: Math.round(fps.noiseFps ?? 3) });
    if (fps.verdict === "better" || fps.verdict === "worse") {
      return t("history.result.fpsLow", { before: Math.round(before), after: Math.round(after) });
    }
  }
  return null;
}

/** Quando a última ação de uma lista rodou com sucesso (Unix s), ou null. */
export function lastActionAt(audit: AuditEvent[], actionNames: string[]): number | null {
  let latest: number | null = null;
  for (const event of audit) {
    const details = auditDetails(event);
    if (!details.success || !details.action_name || !actionNames.includes(details.action_name)) continue;
    if (latest === null || event.timestamp > latest) latest = event.timestamp;
  }
  return latest;
}

const MATCH_WINDOW_SECONDS = 15;

export function buildTimeline(
  snapshots: OptimizationSnapshot[],
  audit: AuditEvent[],
  t: (key: string, params?: Record<string, string | number>) => string,
): TimelineItem[] {
  const commands = audit.filter((event) => event.event === "optimization.command_executed");
  const used = new Set<AuditEvent>();

  const findCommand = (actionName: string, at: number) => {
    let best: AuditEvent | undefined;
    for (const event of commands) {
      if (used.has(event) || auditDetails(event).action_name !== actionName) continue;
      const distance = Math.abs(event.timestamp - at);
      if (distance <= MATCH_WINDOW_SECONDS && (!best || distance < Math.abs(best.timestamp - at))) best = event;
    }
    if (best) used.add(best);
    return best;
  };

  const items: TimelineItem[] = snapshots.map((snapshot) => {
    const command = findCommand(snapshot.action_name, snapshot.created_at);
    const bytes = quarantinedBytes(snapshot);
    return {
      id: `snapshot:${snapshot.id}`,
      at: snapshot.created_at,
      titleKey: actionTitleKey(snapshot.action_name),
      actionName: snapshot.action_name,
      source: SOURCE_BY_AUDIT[auditDetails(command ?? ({} as AuditEvent)).source ?? ""] ?? null,
      snapshotId: snapshot.id,
      restoredAt: snapshot.restored_at ?? null,
      changes: snapshot.entries.map((entry) => describeEntry(entry, t)).filter((change): change is TimelineChange => change !== null),
      result: bytes > 0 ? t("history.result.quarantined", { size: formatBytes(bytes) }) : measuredResult(command, t),
    };
  });

  for (const event of commands) {
    if (used.has(event)) continue;
    const details = auditDetails(event);
    if (!details.success || !details.action_name || !COMMANDS_WITHOUT_SNAPSHOT.has(details.action_name)) continue;
    items.push({
      id: `audit:${event.timestamp}:${details.action_name}`,
      at: event.timestamp,
      titleKey: actionTitleKey(details.action_name),
      actionName: details.action_name,
      source: SOURCE_BY_AUDIT[details.source ?? ""] ?? null,
      snapshotId: null,
      restoredAt: null,
      changes: [],
      result: measuredResult(event, t),
    });
  }

  for (const event of audit) {
    const key = AUTOMATIC_EVENTS[event.event];
    if (!key) continue;
    items.push({
      id: `auto:${event.timestamp}:${event.event}`,
      at: event.timestamp,
      titleKey: key,
      actionName: event.event,
      source: "auto",
      snapshotId: null,
      restoredAt: null,
      changes: [],
      result: null,
    });
  }

  return items.sort((a, b) => b.at - a.at);
}

/** Início do dia local de um Unix em segundos. */
export function startOfDay(unixSeconds: number): number {
  const date = new Date(unixSeconds * 1000);
  date.setHours(0, 0, 0, 0);
  return Math.floor(date.getTime() / 1000);
}
