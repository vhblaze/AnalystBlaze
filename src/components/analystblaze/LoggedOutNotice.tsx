import { useI18n } from "@/i18n";

export function LoggedOutNotice({
  visible,
  busy,
  onLogin,
  errorMessage,
  showManageDevices,
  onManageDevices,
}: {
  visible: boolean;
  busy: boolean;
  onLogin: () => void;
  errorMessage?: string | null;
  showManageDevices?: boolean;
  onManageDevices?: () => void;
}) {
  const { t } = useI18n();
  if (!visible) return null;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("loggedOut.title")}
        className="w-full max-w-md rounded-2xl border border-line bg-bg p-6"
      >
        <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-accent/80">
          {t("loggedOut.eyebrow")}
        </div>
        <h2 className="mt-2 text-xl font-semibold text-fg">{t("loggedOut.title")}</h2>
        <p className="mt-2 text-sm leading-relaxed text-fg-3">{t("loggedOut.description")}</p>
        {errorMessage && (
          <p className="mt-2 rounded-lg border border-danger/30 bg-danger-strong/10 px-3 py-2 text-xs font-medium text-danger-soft">
            {errorMessage}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          {showManageDevices && onManageDevices && (
            <button
              disabled={busy}
              onClick={onManageDevices}
              className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70 disabled:opacity-40"
            >
              {t("loggedOut.manageDevices")}
            </button>
          )}
          <button
            disabled={busy}
            onClick={onLogin}
            className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15 disabled:opacity-50"
          >
            {t("loggedOut.login")}
          </button>
        </div>
      </section>
    </div>
  );
}
