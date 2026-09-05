import { useI18n } from "@/i18n";

export function LoggedOutNotice({
  visible,
  busy,
  onLogin,
  errorMessage,
}: {
  visible: boolean;
  busy: boolean;
  onLogin: () => void;
  errorMessage?: string | null;
}) {
  const { t } = useI18n();
  if (!visible) return null;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("loggedOut.title")}
        className="w-full max-w-md rounded-2xl border border-cyan-400/20 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)]"
      >
        <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-cyan-300/80">
          {t("loggedOut.eyebrow")}
        </div>
        <h2 className="mt-2 text-xl font-semibold text-slate-50">{t("loggedOut.title")}</h2>
        <p className="mt-2 text-sm leading-relaxed text-slate-400">{t("loggedOut.description")}</p>
        {errorMessage && (
          <p className="mt-2 rounded-lg border border-rose-400/30 bg-rose-500/10 px-3 py-2 text-xs font-medium text-rose-200">
            {errorMessage}
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <button
            disabled={busy}
            onClick={onLogin}
            className="rounded-xl border border-cyan-400/50 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15 disabled:opacity-50"
          >
            {t("loggedOut.login")}
          </button>
        </div>
      </section>
    </div>
  );
}
