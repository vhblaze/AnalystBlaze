import { useEffect, useState } from "react";
import { Bell, Search } from "lucide-react";
import type { User } from "@/hooks/useAuth";
import type { AgentStatus } from "@/services/tauri/agent";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";

export function TopBar({ title, user, status }: { title: string; user: User | null; status: AgentStatus | null }) {
  const { t, locale } = useI18n();
  const track = useTelemetry("topbar");
  const [time, setTime] = useState(() => formatTime(locale));
  const isAuthenticated = Boolean(status?.authenticated);
  const isOnline = Boolean(status?.authenticated && status.registered);
  const statusText = !isAuthenticated
    ? t("agent.status.waitingLogin")
    : isOnline
      ? t("topbar.agentOnline")
      : t("agent.status.hardwarePending");

  useEffect(() => {
    setTime(formatTime(locale));
    const id = window.setInterval(() => setTime(formatTime(locale)), 60_000);
    return () => window.clearInterval(id);
  }, [locale]);

  return (
    <header className="relative z-10 flex h-16 shrink-0 items-center justify-between border-b border-cyan-500/10 bg-slate-950/40 px-8 backdrop-blur-xl">
      <div className="flex items-center gap-4">
        <h2 className="text-sm font-medium uppercase tracking-[0.32em] text-slate-400">
          {title}
        </h2>
        <div className="h-4 w-px bg-cyan-500/20" />
        <div className="flex items-center gap-2 text-[11px] font-mono text-slate-500">
          <span className="relative flex h-2 w-2">
            <span className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-70 ${isOnline ? "bg-emerald-400" : "bg-amber-400"}`} />
            <span className={`relative inline-flex h-2 w-2 rounded-full ${isOnline ? "bg-emerald-400" : "bg-amber-400"}`} />
          </span>
          {statusText}
        </div>
      </div>

      <div className="flex items-center gap-3">
        <div className="hidden md:flex items-center gap-2 rounded-xl border border-cyan-500/15 bg-slate-900/50 px-3 py-1.5 text-xs text-slate-500">
          <Search className="h-3.5 w-3.5" />
          <span>{t("topbar.searchPlaceholder")}</span>
          <kbd className="ml-2 rounded border border-cyan-500/20 bg-slate-950/60 px-1.5 py-0.5 text-[10px] font-mono text-slate-400">{t("topbar.commandHint")}</kbd>
        </div>
        <button
          onClick={() => track("notifications_clicked")}
          className="grid h-9 w-9 place-items-center rounded-xl border border-cyan-500/15 bg-slate-900/50 text-slate-400 transition-colors hover:text-cyan-300"
          aria-label={t("topbar.notifications")}
        >
          <Bell className="h-4 w-4" />
        </button>
        <div className="flex items-center gap-2 rounded-xl border border-cyan-500/15 bg-slate-900/50 px-3 py-1.5 font-mono text-xs text-cyan-300/80">
          {time}
        </div>
        {user && (
          <div className="grid h-9 w-9 place-items-center rounded-full border border-cyan-400/40 bg-gradient-to-br from-cyan-500/30 to-violet-500/20 text-xs font-semibold text-cyan-100">
            {user.name.slice(0, 1).toUpperCase()}
          </div>
        )}
      </div>
    </header>
  );
}

function formatTime(locale: string) {
  return new Date().toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
}
