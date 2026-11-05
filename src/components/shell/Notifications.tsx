import { useEffect, useMemo, useRef, useState } from "react";
import { Bell, Inbox } from "lucide-react";
import type { User } from "@/hooks/useAuth";
import { isUpdateDismissedNow, type useUpdater } from "@/hooks/useUpdater";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { cn } from "@/lib/utils";
import type { Announcement, RemoteCommandConfirmationRequest } from "@/services/tauri/agent";

export type NotificationItem = {
  id: string;
  title: string;
  description?: string;
  tone?: "info" | "warning" | "danger";
  createdAt?: string;
};

/** O que aparece no sino: pedidos remotos, e-mail a verificar, atualização e anúncios. */
export function useNotificationItems({
  remoteConfirmationQueue,
  user,
  updateStatus,
  announcements,
  dismissedAnnouncementIds,
}: {
  remoteConfirmationQueue: RemoteCommandConfirmationRequest[];
  user: User | null;
  updateStatus: ReturnType<typeof useUpdater>["status"];
  announcements: Announcement[];
  dismissedAnnouncementIds: string[];
}) {
  const { t } = useI18n();
  return useMemo<NotificationItem[]>(() => {
    const items: NotificationItem[] = remoteConfirmationQueue.map((request) => ({
      id: request.requestId,
      title: request.title || request.actionName,
      description: request.description,
      tone: "warning",
    }));
    if (!user?.emailVerified) {
      const days = user?.emailVerificationDaysRemaining ?? null;
      items.unshift({
        id: "email-verification",
        title: t("emailVerification.bellTitle"),
        description:
          typeof days === "number"
            ? t("emailVerification.bellDescriptionWithDays", { days })
            : t("emailVerification.bellDescription"),
        tone: typeof days === "number" && days <= 7 ? "danger" : "warning",
      });
    }
    const update = updateStatus;
    if (update?.available && (update.mandatory || !isUpdateDismissedNow(update))) {
      items.unshift({
        id: "update-available",
        title: update.mandatory
          ? t("update.availableTitleMandatory", { version: update.version ?? "" })
          : t("update.availableTitle", { version: update.version ?? "" }),
        description: update.notes?.trim() || t("update.notesFallback"),
        tone: update.mandatory ? "danger" : "info",
      });
    }
    announcements
      .filter((announcement) => !dismissedAnnouncementIds.includes(announcement.id))
      .forEach((announcement) => {
        items.push({
          id: `announcement:${announcement.id}`,
          title: announcement.title,
          description: announcement.body,
          tone: announcement.tone,
          createdAt: announcement.createdAt,
        });
      });
    return items;
  }, [remoteConfirmationQueue, updateStatus, announcements, dismissedAnnouncementIds, user, t]);
}

const toneDot = {
  danger: "bg-danger",
  warning: "bg-warn",
  info: "bg-accent",
} as const;

/** O sino do header (saiu do TopBar/AppShell). */
export function Notifications({
  items,
  onItemClick,
}: {
  items: NotificationItem[];
  onItemClick: (id: string) => void;
}) {
  const { t } = useI18n();
  const track = useTelemetry("topbar");
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!boxRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const count = items.length;

  return (
    <div ref={boxRef} className="relative">
      <button
        type="button"
        onClick={() => {
          track("notifications_clicked", { pending: count });
          setOpen((value) => !value);
        }}
        className="relative grid h-[30px] w-[30px] place-items-center rounded-control border border-line text-fg-3 transition-colors duration-fast ease-out hover:border-fg-4/60 hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        aria-label={count > 0 ? `${t("topbar.notifications")} (${count})` : t("topbar.notifications")}
        aria-expanded={open}
      >
        <Bell className="h-4 w-4" />
        {count > 0 && (
          <span className="num absolute -right-1.5 -top-1.5 grid h-4 min-w-4 place-items-center rounded-pill bg-warn px-1 text-[10px] font-bold leading-none text-bg">
            {count > 9 ? "9+" : count}
          </span>
        )}
      </button>
      {open && (
        <div className="absolute right-0 top-10 z-40 w-80 overflow-hidden rounded-card border border-line bg-surface shadow-xl shadow-black/40">
          <div className="label-mono border-b border-line px-4 py-3">{t("topbar.notifications")}</div>
          {count > 0 ? (
            <div className="max-h-80 overflow-y-auto p-1.5">
              {items.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => {
                    track("notification_item_clicked", { id: item.id });
                    onItemClick(item.id);
                    setOpen(false);
                  }}
                  className="flex w-full items-start gap-3 rounded-control px-3 py-2.5 text-left transition-colors duration-fast hover:bg-surface-2"
                >
                  <span className={cn("mt-1.5 h-2 w-2 shrink-0 rounded-pill", toneDot[item.tone ?? "info"])} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium text-fg">{item.title}</span>
                    {item.description && (
                      <span className="mt-0.5 line-clamp-2 block text-xs text-fg-3">{item.description}</span>
                    )}
                  </span>
                </button>
              ))}
            </div>
          ) : (
            <div className="flex items-center gap-2 px-4 py-4 text-sm text-fg-3">
              <Inbox className="h-4 w-4" />
              {t("topbar.notificationsEmpty")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
