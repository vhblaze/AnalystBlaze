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
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-md rounded-2xl border border-cyan-400/20 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)]"
      >
        <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-cyan-300/80">
          {t("update.eyebrow")}
        </div>
        <h2 className="mt-2 text-xl font-semibold text-slate-50">{title}</h2>
        <p className="mt-2 text-sm leading-relaxed text-slate-400">
          {status.notes?.trim() || t("update.notesFallback")}
        </p>
        {mandatory && (
          <p className="mt-2 text-xs font-medium text-rose-200">{t("update.mandatoryNotice")}</p>
        )}
        {!status.downloaded && !status.installing && (
          <p className="mt-2 text-xs text-slate-500">{t("update.downloading")}</p>
        )}
        {status.lastError && (
          <p className="mt-2 text-xs font-medium text-rose-300">{status.lastError}</p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          {!mandatory && (
            <button
              disabled={busy}
              onClick={onLater}
              className="rounded-xl border border-slate-600/60 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-400/70 disabled:opacity-40"
            >
              {t("update.later")}
            </button>
          )}
          <button
            disabled={busy || status.installing}
            onClick={onUpdateNow}
            className="rounded-xl border border-cyan-400/50 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
          >
            {status.installing ? t("update.installing") : t("update.updateNow")}
          </button>
        </div>
      </section>
    </div>
  );
}
