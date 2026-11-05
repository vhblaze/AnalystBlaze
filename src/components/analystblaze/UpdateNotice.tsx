import { X } from "lucide-react";
import { isUpdateDismissedNow } from "@/hooks/useUpdater";
import { useI18n } from "@/i18n";
import type { UpdateStatus } from "@/services/tauri/agent";

export function UpdateNotice({
  status,
  busy,
  onUpdateNow,
  onLater,
}: {
  status: UpdateStatus | null;
  busy: boolean;
  onUpdateNow: () => void;
  onLater: () => void;
}) {
  const { t } = useI18n();
  if (!status?.available) return null;
  if (!status.mandatory && isUpdateDismissedNow(status)) return null;

  const mandatory = status.mandatory;
  const title = mandatory
    ? t("update.availableTitleMandatory", { version: status.version ?? "" })
    : t("update.availableTitle", { version: status.version ?? "" });

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={title}
        // max-h + flex column is what actually fixes this: a long changelog
        // used to push the action buttons off the bottom of the screen with
        // nothing to scroll and nothing to close, on any display shorter
        // than the notes needed - real users got stuck unable to update at
        // all. The header/notes area scrolls on its own; the button row sits
        // outside that scroll region so it's always visible, never something
        // you have to scroll past a wall of text to reach.
        className="relative flex max-h-[85vh] w-full max-w-md flex-col rounded-2xl border border-line bg-bg"
      >
        {/* Closing (like "Depois") only ever snoozes this popup - it never
            skips a mandatory update. The backend re-surfaces a mandatory one
            regardless of the dismiss timer (see should_surface_update_window
            in updater.rs), so this is purely an escape hatch for whoever
            can't otherwise reach the buttons below, not a way around a
            required update. */}
        <button
          type="button"
          onClick={onLater}
          disabled={busy}
          aria-label={t("update.close")}
          className="absolute right-3 top-3 z-10 rounded-lg p-1.5 text-fg-3 transition hover:bg-white/5 hover:text-fg disabled:opacity-40"
        >
          <X className="h-4 w-4" />
        </button>

        <div className="min-h-0 flex-1 overflow-y-auto p-6 pb-4 pr-10">
          <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-accent/80">
            {t("update.eyebrow")}
          </div>
          <h2 className="mt-2 text-xl font-semibold text-fg">{title}</h2>
          <p className="mt-2 whitespace-pre-line text-sm leading-relaxed text-fg-3">
            {status.notes?.trim() || t("update.notesFallback")}
          </p>
          {mandatory && (
            <p className="mt-2 text-xs font-medium text-danger-soft">{t("update.mandatoryNotice")}</p>
          )}
          {!status.downloaded && !status.installing && (
            <p className="mt-2 text-xs text-fg-4">{t("update.downloading")}</p>
          )}
          {status.lastError && (
            <p className="mt-2 text-xs font-medium text-danger">{status.lastError}</p>
          )}
          {!status.installing && (
            <p className="mt-2 text-xs text-fg-4">{t("update.elevationNotice")}</p>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-white/5 p-4">
          {!mandatory && (
            <button
              disabled={busy}
              onClick={onLater}
              className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70 disabled:opacity-40"
            >
              {t("update.later")}
            </button>
          )}
          <button
            disabled={busy || status.installing}
            onClick={onUpdateNow}
            className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15 disabled:opacity-50"
          >
            {status.installing ? t("update.installing") : t("update.updateNow")}
          </button>
        </div>
      </section>
    </div>
  );
}
