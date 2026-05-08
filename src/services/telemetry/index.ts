type TelemetryPrimitive = string | number | boolean | null;
type TelemetryProperties = Record<string, unknown>;

export type TelemetryEventInput = {
  name: string;
  category?: string;
  properties?: TelemetryProperties;
};

export type TelemetryEvent = {
  id: string;
  name: string;
  category: string;
  timestamp: number;
  sessionId: string;
  appVersion: string;
  path: string;
  properties: Record<string, TelemetryPrimitive | TelemetryPrimitive[] | Record<string, TelemetryPrimitive>>;
};

const QUEUE_KEY = "analystblaze.uiTelemetry.queue";
const ENABLED_KEY = "analystblaze.uiTelemetry.enabled";
const SESSION_KEY = "analystblaze.uiTelemetry.session";
const DEFAULT_FLUSH_INTERVAL_MS = 60 * 60 * 1000;
const DEFAULT_MAX_EVENTS = 500;
const MAX_BATCH_SIZE = 100;
const SENSITIVE_KEY = /(token|secret|password|authorization|email|name|hw_id|hardware|jwt|credential)/i;

let intervalId: number | undefined;
let sending = false;
let onlineHandler: (() => void) | undefined;
let visibilityHandler: (() => void) | undefined;

const sessionId = readSessionId();

function numberFromEnv(value: string | undefined, fallback: number, min: number, max: number): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, parsed));
}

function endpoint() {
  return import.meta.env.VITE_ANALYSTBLAZE_TELEMETRY_URL?.trim();
}

function flushIntervalMs() {
  return numberFromEnv(
    import.meta.env.VITE_ANALYSTBLAZE_TELEMETRY_FLUSH_INTERVAL_MS,
    DEFAULT_FLUSH_INTERVAL_MS,
    60_000,
    24 * 60 * 60 * 1000,
  );
}

function maxEvents() {
  return numberFromEnv(import.meta.env.VITE_ANALYSTBLAZE_TELEMETRY_MAX_EVENTS, DEFAULT_MAX_EVENTS, 50, 5_000);
}

function appVersion() {
  return import.meta.env.VITE_ANALYSTBLAZE_APP_VERSION?.trim() || "0.1.0";
}

function readSessionId() {
  try {
    const existing = sessionStorage.getItem(SESSION_KEY);
    if (existing) return existing;
    const next = createId();
    sessionStorage.setItem(SESSION_KEY, next);
    return next;
  } catch {
    return createId();
  }
}

function createId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function readQueue(): TelemetryEvent[] {
  try {
    const raw = localStorage.getItem(QUEUE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writeQueue(events: TelemetryEvent[]) {
  const trimmed = events.slice(-maxEvents());
  try {
    localStorage.setItem(QUEUE_KEY, JSON.stringify(trimmed));
  } catch {
    try {
      localStorage.setItem(QUEUE_KEY, JSON.stringify(trimmed.slice(-Math.floor(maxEvents() / 2))));
    } catch {
      // Storage quota errors should never affect the interface.
    }
  }
}

function sanitizeValue(value: unknown, depth = 0): TelemetryPrimitive | TelemetryPrimitive[] | Record<string, TelemetryPrimitive> {
  if (value == null) return null;
  if (typeof value === "string") return value.slice(0, 200);
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "boolean") return value;

  if (depth > 1) return "[object]";

  if (Array.isArray(value)) {
    return value.slice(0, 10).map((item) => {
      const sanitized = sanitizeValue(item, depth + 1);
      return typeof sanitized === "object" ? "[object]" : sanitized;
    });
  }

  if (typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .slice(0, 20)
        .map(([key, item]) => [key, SENSITIVE_KEY.test(key) ? "[redacted]" : primitiveOnly(sanitizeValue(item, depth + 1))]),
    );
  }

  return String(value).slice(0, 120);
}

function primitiveOnly(value: ReturnType<typeof sanitizeValue>): TelemetryPrimitive {
  if (Array.isArray(value)) return `[array:${value.length}]`;
  if (value && typeof value === "object") return "[object]";
  return value as TelemetryPrimitive;
}

function sanitizeProperties(properties?: TelemetryProperties): TelemetryEvent["properties"] {
  if (!properties) return {};
  return sanitizeValue(properties) as TelemetryEvent["properties"];
}

export function isTelemetryEnabled() {
  try {
    return localStorage.getItem(ENABLED_KEY) !== "0";
  } catch {
    return true;
  }
}

export function setTelemetryEnabled(enabled: boolean) {
  try {
    localStorage.setItem(ENABLED_KEY, enabled ? "1" : "0");
  } catch {
    // Preference persistence is best effort.
  }
}

export function getTelemetryQueueSize() {
  return readQueue().length;
}

export function captureTelemetry(input: TelemetryEventInput) {
  if (!isTelemetryEnabled()) return;

  const event: TelemetryEvent = {
    id: createId(),
    name: input.name,
    category: input.category ?? "ui",
    timestamp: Date.now(),
    sessionId,
    appVersion: appVersion(),
    path: `${location.pathname}${location.hash}`,
    properties: sanitizeProperties(input.properties),
  };

  writeQueue([...readQueue(), event]);
}

function isFlushDue(queue = readQueue()) {
  if (queue.length === 0) return false;
  return Date.now() - queue[0].timestamp >= flushIntervalMs();
}

export async function flushTelemetry(reason = "interval") {
  const url = endpoint();
  if (!url || sending || !isTelemetryEnabled()) return false;
  if (typeof navigator !== "undefined" && navigator.onLine === false) return false;

  const queue = readQueue();
  if (queue.length === 0 || !isFlushDue(queue)) return false;

  const batch = queue.slice(0, MAX_BATCH_SIZE);
  sending = true;
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        sentAt: new Date().toISOString(),
        reason,
        events: batch,
      }),
      keepalive: batch.length <= 20,
    });

    if (!response.ok) return false;

    const sentIds = new Set(batch.map((event) => event.id));
    writeQueue(readQueue().filter((event) => !sentIds.has(event.id)));
    return true;
  } catch {
    return false;
  } finally {
    sending = false;
  }
}

export function startTelemetryService() {
  if (intervalId !== undefined) {
    return stopTelemetryService;
  }

  const tryFlush = (reason: string) => {
    if (isFlushDue()) {
      void flushTelemetry(reason);
    }
  };

  intervalId = window.setInterval(() => tryFlush("interval"), flushIntervalMs());
  onlineHandler = () => tryFlush("online");
  visibilityHandler = () => {
    if (document.visibilityState === "hidden") {
      tryFlush("visibility_hidden");
    }
  };

  window.addEventListener("online", onlineHandler);
  document.addEventListener("visibilitychange", visibilityHandler);

  window.setTimeout(() => tryFlush("startup"), 2_000);

  return stopTelemetryService;
}

export function stopTelemetryService() {
  if (intervalId !== undefined) {
    window.clearInterval(intervalId);
    intervalId = undefined;
  }
  if (onlineHandler) {
    window.removeEventListener("online", onlineHandler);
    onlineHandler = undefined;
  }
  if (visibilityHandler) {
    document.removeEventListener("visibilitychange", visibilityHandler);
    visibilityHandler = undefined;
  }
}

export function captureUiError(error: unknown, source: string) {
  captureTelemetry({
    name: "ui_error",
    category: "error",
    properties: {
      source,
      message: error instanceof Error ? error.message : String(error),
    },
  });
}
