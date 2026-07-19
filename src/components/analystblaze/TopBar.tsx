import { useEffect, useMemo, useRef, useState } from "react";
import { Bell, Inbox, Search } from "lucide-react";
import type { User } from "@/hooks/useAuth";
import type { AgentStatus } from "@/services/tauri/agent";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";

export type TopBarSearchItem = {
  id: string;
  title: string;
  description?: string;
  keywords?: string[];
  hint?: string;
  disabled?: boolean;
  onSelect: () => void | Promise<void>;
};

export type TopBarNotification = {
  id: string;
  title: string;
  description?: string;
  tone?: "info" | "warning" | "danger";
  createdAt?: string;
};

export function TopBar({
  title,
  user,
  status,
  pendingNotifications = 0,
  notifications = [],
  onNotificationsClick,
  onNotificationClick,
  searchItems = [],
}: {
  title: string;
  user: User | null;
  status: AgentStatus | null;
  pendingNotifications?: number;
  notifications?: TopBarNotification[];
  onNotificationsClick?: () => void;
  onNotificationClick?: (id: string) => void;
  searchItems?: TopBarSearchItem[];
}) {
  const { t, locale } = useI18n();
  const track = useTelemetry("topbar");
  const [time, setTime] = useState(() => formatTime(locale));
  const [searchQuery, setSearchQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const searchBoxRef = useRef<HTMLDivElement | null>(null);
  const notificationBoxRef = useRef<HTMLDivElement | null>(null);
  const isAuthenticated = Boolean(status?.authenticated);
  const isOnline = Boolean(status?.authenticated && status.registered);
  const notificationCount = Math.max(pendingNotifications, notifications.length);
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

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearchOpen(true);
        searchInputRef.current?.focus();
      }
      if (event.key === "Escape") {
        setSearchOpen(false);
        setNotificationsOpen(false);
        searchInputRef.current?.blur();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && !searchBoxRef.current?.contains(target)) {
        setSearchOpen(false);
      }
      if (target && !notificationBoxRef.current?.contains(target)) {
        setNotificationsOpen(false);
      }
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, []);

  const filteredSearchItems = useMemo(() => {
    const query = normalizeSearch(searchQuery);
    if (!query) return searchItems.slice(0, 7);
    return searchItems
      .filter((item) => {
        const haystack = normalizeSearch(
          [item.title, item.description, item.hint, ...(item.keywords ?? [])].filter(Boolean).join(" "),
        );
        return haystack.includes(query);
      })
      .slice(0, 7);
  }, [searchItems, searchQuery]);

  const selectSearchItem = async (item: TopBarSearchItem) => {
    if (item.disabled) return;
    track("search_item_selected", { id: item.id });
    setSearchQuery("");
    setSearchOpen(false);
    searchInputRef.current?.blur();
    await item.onSelect();
  };

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
        <div ref={searchBoxRef} className="relative hidden md:block">
          <div className="flex h-9 min-w-[280px] items-center gap-2 rounded-xl border border-cyan-500/15 bg-slate-900/50 px-3 text-xs text-slate-500 transition focus-within:border-cyan-400/50 focus-within:text-cyan-100">
            <Search className="h-3.5 w-3.5 shrink-0" />
            <input
              ref={searchInputRef}
              value={searchQuery}
              onChange={(event) => {
                setSearchQuery(event.target.value);
                setSearchOpen(true);
              }}
              onFocus={() => setSearchOpen(true)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && filteredSearchItems[0]) {
                  event.preventDefault();
                  void selectSearchItem(filteredSearchItems[0]);
                }
              }}
              placeholder={t("topbar.searchPlaceholder")}
              className="min-w-0 flex-1 bg-transparent text-xs text-slate-200 outline-none placeholder:text-slate-500"
            />
            <kbd className="ml-2 shrink-0 rounded border border-cyan-500/20 bg-slate-950/60 px-1.5 py-0.5 text-[10px] font-mono text-slate-400">
              {t("topbar.commandHint")}
            </kbd>
          </div>
          {searchOpen && (
            <div className="absolute right-0 top-11 z-40 w-[360px] overflow-hidden rounded-xl border border-cyan-500/20 bg-slate-950/95 shadow-[0_24px_70px_-32px_hsl(187_100%_55%/0.8)] backdrop-blur-xl">
              {filteredSearchItems.length > 0 ? (
                <div className="max-h-80 overflow-y-auto p-1.5">
                  {filteredSearchItems.map((item) => (
                    <button
                      key={item.id}
                      disabled={item.disabled}
                      onClick={() => void selectSearchItem(item)}
                      className="flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition hover:bg-cyan-400/10 disabled:cursor-not-allowed disabled:opacity-45"
                    >
                      <Search className="mt-0.5 h-3.5 w-3.5 shrink-0 text-cyan-300" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium text-slate-100">{item.title}</span>
                        {item.description && (
                          <span className="mt-0.5 block line-clamp-2 text-xs text-slate-500">{item.description}</span>
                        )}
                      </span>
                      {item.hint && (
                        <span className="shrink-0 rounded-md border border-cyan-500/15 bg-cyan-500/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-widest text-cyan-200">
                          {item.hint}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              ) : (
                <div className="flex items-center gap-2 px-4 py-4 text-sm text-slate-500">
                  <Inbox className="h-4 w-4 text-slate-600" />
                  {t("topbar.searchEmpty")}
                </div>
              )}
            </div>
          )}
        </div>

        <div ref={notificationBoxRef} className="relative">
          <button
            onClick={() => {
              track("notifications_clicked", { pending: notificationCount });
              setNotificationsOpen((open) => !open);
              onNotificationsClick?.();
            }}
            className="relative grid h-9 w-9 place-items-center rounded-xl border border-cyan-500/15 bg-slate-900/50 text-slate-400 transition-colors hover:text-cyan-300"
            aria-label={t("topbar.notifications")}
          >
            <Bell className="h-4 w-4" />
            {notificationCount > 0 && (
              <span className="absolute -right-1 -top-1 grid h-4 min-w-4 place-items-center rounded-full border border-slate-950 bg-amber-300 px-1 text-[10px] font-bold leading-none text-slate-950">
                {notificationCount > 9 ? "9+" : notificationCount}
              </span>
            )}
          </button>
          {notificationsOpen && (
            <div className="absolute right-0 top-11 z-40 w-80 overflow-hidden rounded-xl border border-cyan-500/20 bg-slate-950/95 shadow-[0_24px_70px_-32px_hsl(187_100%_55%/0.8)] backdrop-blur-xl">
              <div className="border-b border-cyan-500/10 px-4 py-3">
                <div className="font-mono text-[10px] uppercase tracking-[0.22em] text-cyan-300/80">
                  {t("topbar.notifications")}
                </div>
              </div>
              {notifications.length > 0 ? (
                <div className="max-h-80 overflow-y-auto p-1.5">
                  {notifications.map((notification) => (
                    <button
                      key={notification.id}
                      onClick={() => {
                        track("notification_item_clicked", { id: notification.id });
                        onNotificationClick?.(notification.id);
                        setNotificationsOpen(false);
                      }}
                      className="flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition hover:bg-cyan-400/10"
                    >
                      <span className={`mt-1 h-2 w-2 shrink-0 rounded-full ${notification.tone === "danger" ? "bg-rose-300" : notification.tone === "warning" ? "bg-amber-300" : "bg-cyan-300"}`} />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium text-slate-100">{notification.title}</span>
                        {notification.description && (
                          <span className="mt-0.5 block line-clamp-2 text-xs text-slate-500">{notification.description}</span>
                        )}
                      </span>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="flex items-center gap-2 px-4 py-4 text-sm text-slate-500">
                  <Inbox className="h-4 w-4 text-slate-600" />
                  {t("topbar.notificationsEmpty")}
                </div>
              )}
            </div>
          )}
        </div>
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

function normalizeSearch(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .trim();
}
