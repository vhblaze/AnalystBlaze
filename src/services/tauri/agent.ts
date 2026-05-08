import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";

export type AgentStatus = {
  authenticated: boolean;
  registered: boolean;
  hw_id?: string | null;
  user_name?: string | null;
  user_email?: string | null;
  plan: string;
  has_paid_plan: boolean;
  mode: string;
  api_base_url: string;
  web_login_url: string;
};

export type AgentTelemetrySample = {
  event_timestamp: number;
  cpu_usage: number;
  gpu_usage: number;
  gpu_name?: string;
  vram_gb?: number;
  ram_usage_mb: number;
  gpu_temperature: number;
  latency_ms: number;
};

type SingleInstancePayload = {
  args?: unknown[];
  cwd?: string;
};

const fallbackStatus: AgentStatus = {
  authenticated: false,
  registered: false,
  hw_id: null,
  user_name: null,
  user_email: null,
  plan: "starter",
  has_paid_plan: false,
  mode: "stopped",
  api_base_url: import.meta.env.VITE_ANALYSTBLAZE_API_URL ?? "http://localhost:8000",
  web_login_url: import.meta.env.VITE_ANALYSTBLAZE_WEB_LOGIN_URL ?? "http://localhost:3000/login",
};

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getAgentStatus() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("agent_status");
}

export async function openAgentLogin() {
  if (!isTauriRuntime()) return fallbackStatus.web_login_url;
  return invoke<string>("open_login");
}

export async function completeAuthFromDeepLink(rawUrl: string) {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("complete_auth_from_deep_link", { rawUrl });
}

export async function startAgent() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("start_agent");
}

export async function setAgentTelemetryMode(mode: "normal" | "realtime") {
  if (!isTauriRuntime()) return { ...fallbackStatus, mode };
  return invoke<AgentStatus>("set_telemetry_mode", { mode });
}

export async function logoutAgent() {
  if (!isTauriRuntime()) return fallbackStatus;
  return invoke<AgentStatus>("logout");
}

export async function collectAgentTelemetrySample(): Promise<AgentTelemetrySample> {
  if (!isTauriRuntime()) {
    return {
      event_timestamp: Math.floor(Date.now() / 1000),
      cpu_usage: 18 + Math.random() * 20,
      gpu_usage: 10 + Math.random() * 25,
      gpu_name: "Desktop preview",
      vram_gb: 0,
      ram_usage_mb: 3_200 + Math.random() * 1_000,
      gpu_temperature: 42 + Math.random() * 8,
      latency_ms: 12 + Math.random() * 10,
    };
  }
  return invoke<AgentTelemetrySample>("collect_once");
}

export async function registerDeepLinkHandlers(onUrl: (url: string) => void) {
  if (!isTauriRuntime()) return () => undefined;

  const disposers: Array<() => void> = [];
  const handleUrls = (urls: Iterable<string>) => {
    for (const url of urls) {
      if (isAuthDeepLink(url)) onUrl(url);
    }
  };

  try {
    const urls = await getCurrent();
    handleUrls(urls ?? []);
  } catch {
    // Deep link startup payload is optional.
  }

  try {
    disposers.push(
      await onOpenUrl((urls) => {
        handleUrls(urls);
      }),
    );
  } catch {
    // Runtime deep link events are unavailable in some dev environments.
  }

  try {
    disposers.push(
      await listen<SingleInstancePayload>("single-instance", (event) => {
        handleUrls(extractDeepLinks(event.payload?.args));
      }),
    );
  } catch {
    // The single-instance plugin is a fallback for platforms that pass the URL as an argv.
  }

  return () => {
    for (const dispose of disposers) dispose();
  };
}

function extractDeepLinks(args: unknown[] | undefined) {
  if (!Array.isArray(args)) return [];
  return args.filter((value): value is string => typeof value === "string" && isAuthDeepLink(value));
}

function isAuthDeepLink(rawUrl: string) {
  try {
    const url = new URL(rawUrl);
    return url.protocol === "analystblaze:" && (url.hostname === "auth" || url.pathname.replace(/^\/+|\/+$/g, "") === "auth");
  } catch {
    return false;
  }
}
