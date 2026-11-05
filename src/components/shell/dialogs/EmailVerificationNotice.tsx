import { useI18n } from "@/i18n";

/**
 * Aviso de e-mail nao verificado. A verificacao em si (pedir o codigo e
 * digitar os 6 digitos) acontece no site: os endpoints sao publicos e a conta
 * pode ate estar desativada, entao o navegador e o caminho que funciona nos
 * dois casos - aqui o app so avisa e leva pra la.
 *
 * "Agora não" fecha por esta sessao do app, nao dispensa o aviso: o item no
 * sininho continua la, e o popup volta na proxima abertura enquanto o e-mail
 * nao for verificado. Esconder isso de vez seria esconder a desativacao.
 */
export function EmailVerificationNotice({
  daysRemaining,
  onVerify,
  onLater,
}: {
  daysRemaining: number | null;
  onVerify: () => void;
  onLater: () => void;
}) {
  const { t } = useI18n();
  const isUrgent = typeof daysRemaining === "number" && daysRemaining <= 7;

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("emailVerification.popupTitle")}
        className={`w-full max-w-lg rounded-2xl border bg-bg p-6 ${
          isUrgent ? "border-danger/40" : "border-warn/30"
        }`}
      >
        <div
          className={`font-mono text-[10px] uppercase tracking-[0.25em] ${
            isUrgent ? "text-danger" : "text-warn"
          }`}
        >
          {t("emailVerification.bellTitle")}
        </div>
        <h2 className="mt-2 text-xl font-semibold text-fg">{t("emailVerification.popupTitle")}</h2>
        <p className="mt-2 text-sm leading-relaxed text-fg-3">
          {typeof daysRemaining === "number"
            ? t("emailVerification.popupBodyWithDays", { days: daysRemaining })
            : t("emailVerification.popupBody")}
        </p>
        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onLater}
            className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70"
          >
            {t("emailVerification.laterAction")}
          </button>
          <button
            onClick={onVerify}
            className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15"
          >
            {t("emailVerification.verifyAction")}
          </button>
        </div>
      </section>
    </div>
  );
}
