import { useI18n } from "@/i18n";
import type { DeviceChoice } from "@/services/tauri/agent";

/** Bloco 5.4: the account already has its 2 active PCs. The server no longer
 * switches one off on its own, so the person at this PC picks which one it
 * replaces. Same frame as ConfirmationDialog. */
export function DeviceLimitDialog({
  devices,
  busy,
  onReplace,
  onCancel,
}: {
  devices: DeviceChoice[];
  busy: boolean;
  onReplace: (hwId: string) => void;
  onCancel: () => void;
}) {
  const { t, locale } = useI18n();
  const describe = (device: DeviceChoice) =>
    [device.machine_type, device.os_version].filter(Boolean).join(" · ");
  const lastSeen = (value?: string | null) => {
    if (!value) return null;
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return null;
    return t("deviceLimit.lastSeen", {
      date: date.toLocaleString(locale, { dateStyle: "short", timeStyle: "short" }),
    });
  };
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("deviceLimit.title")}
        className="relative flex max-h-[85vh] w-full max-w-lg flex-col rounded-2xl border border-line bg-bg"
      >
        <div className="min-h-0 flex-1 overflow-y-auto p-6 pb-4">
          <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-warn">
            {t("deviceLimit.eyebrow")}
          </div>
          <h2 className="mt-2 text-xl font-semibold text-fg">{t("deviceLimit.title")}</h2>
          <p className="mt-2 text-sm leading-relaxed text-fg-3">{t("deviceLimit.body")}</p>
          <ul className="mt-4 grid gap-2">
            {devices.map((device) => (
              <li
                key={device.id}
                className="flex items-center justify-between gap-3 rounded-xl border border-line bg-surface/70 p-3"
              >
                <div className="min-w-0">
                  <strong className="block truncate text-sm text-fg">
                    {device.name || t("deviceLimit.unnamed")}
                  </strong>
                  <span className="block truncate text-xs text-fg-4">{describe(device)}</span>
                  {lastSeen(device.last_ping) && (
                    <span className="block text-xs text-fg-4">{lastSeen(device.last_ping)}</span>
                  )}
                </div>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => onReplace(device.id)}
                  className="shrink-0 rounded-xl border border-fg-4/60 px-3 py-1.5 text-sm font-medium text-fg transition hover:border-fg-3/70 disabled:opacity-50"
                >
                  {t("deviceLimit.replace")}
                </button>
              </li>
            ))}
          </ul>
        </div>
        <div className="flex justify-end gap-2 border-t border-white/5 p-4">
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70 disabled:opacity-50"
          >
            {t("deviceLimit.cancel")}
          </button>
        </div>
      </section>
    </div>
  );
}
