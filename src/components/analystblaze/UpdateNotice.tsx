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

  const tone = status.mandatory
    ? "border-rose-300/30 bg-rose-400/10 text-rose-50"
    : "border-cyan-300/30 bg-cyan-400/10 text-cyan-50";
  const title = status.mandatory
    ? t("update.availableTitleMandatory", { version: status.version ?? "" })
    : t("update.availableTitle", { version: status.version ?? "" });

  return (
    <div className={`mb-4 rounded-xl border px-4 py-3 text-sm shadow-[0_18px_50px_-32px_hsl(187_100%_55%/0.5)] ${tone}`}>
      <div className="font-mono text-[10px] uppercase tracking-[0.24em] opacity-80">
        {t("update.eyebrow")}
      </div>
      <div className="mt-1 font-semibold">{title}</div>
      <p className="mt-1 max-w-3xl text-xs leading-relaxed opacity-80">
        {status.notes?.trim() || t("update.notesFallback")}
      </p>
      {status.mandatory && (
        <p className="mt-1 text-xs font-medium">{t("update.mandatoryNotice")}</p>
      )}
      {!status.downloaded && !status.installing && (
        <p className="mt-1 text-xs opacity-70">{t("update.downloading")}</p>
      )}
      {status.lastError && (
        <p className="mt-1 text-xs font-medium text-rose-100">{status.lastError}</p>
      )}
      <div className="mt-3 flex gap-2">
        <button
          disabled={busy || status.installing}
          onClick={onUpdateNow}
          className="rounded-lg border border-current/40 bg-white/10 px-3 py-1.5 text-xs font-semibold transition hover:bg-white/15 disabled:opacity-50"
        >
          {status.installing ? t("update.installing") : t("update.updateNow")}
        </button>
        {!status.mandatory && (
          <button
            disabled={busy}
            onClick={onLater}
            className="rounded-lg border border-current/20 px-3 py-1.5 text-xs font-medium opacity-80 transition hover:opacity-100 disabled:opacity-40"
          >
            {t("update.later")}
          </button>
        )}
      </div>
    </div>
  );
}
