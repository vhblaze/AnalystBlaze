import { useState } from "react";
import { Shield, X, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { toast } from "@/hooks/use-toast";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { errorText } from "@/features/shared/agentResource";
import { protectApp, unprotectApp, useProtectedApps } from "@/features/windows/useWindowsInventory";

/**
 * Controles › Processos protegidos: apps em que o AnalystBlaze nunca mexe.
 * Os padrões do agente (created_at = 0) não saem da lista.
 */
export function ProtectedApps() {
  const { t } = useI18n();
  const track = useTelemetry("controls_protected");
  const { apps, loading, refresh } = useProtectedApps();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    const name = input.trim();
    if (!name) return;
    setBusy(true);
    try {
      await protectApp(name);
      setInput("");
      track("protected_app_added", { name });
      toast({ title: t("controls.protectedAdded", { name }) });
    } catch (error) {
      toast({ title: t("controls.d3.actionFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setBusy(false);
    }
  };

  const remove = async (name: string) => {
    setBusy(true);
    try {
      await unprotectApp(name);
      track("protected_app_removed", { name });
      toast({ title: t("controls.protectedRemoved", { name }) });
    } catch (error) {
      toast({ title: t("controls.d3.actionFailed"), description: errorText(error), variant: "destructive" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("controls.d3.protected.title")}</h2>
          <p className="mt-1 text-sm text-fg-3">{t("controls.d3.protected.subtitle")}</p>
        </div>
        <Button size="sm" variant="outline" disabled={loading || busy} onClick={() => void refresh().catch(() => undefined)}>
          <RefreshCw />
          {t("common.refresh")}
        </Button>
      </header>
      <Card>
        <form
          className="flex gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void add();
          }}
        >
          <input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder={t("settings.protectedAppPlaceholder")}
            aria-label={t("controls.d3.protected.inputLabel")}
            className="h-9 min-w-0 flex-1 rounded-control border border-line bg-bg/60 px-3 text-sm text-fg outline-none transition-colors focus:border-accent/60"
          />
          <Button type="submit" variant="outline" disabled={busy || !input.trim()}>
            {t("settings.addProtectedApp")}
          </Button>
        </form>
        <ul className="mt-4 flex flex-wrap gap-2">
          {apps.length === 0 && <li className="text-sm text-fg-3">{loading ? t("controls.d3.measuring") : t("controls.d3.protected.empty")}</li>}
          {apps.map((app) => {
            const isDefault = app.created_at <= 0;
            return (
              <li
                key={`${app.name}:${app.created_at}`}
                className="inline-flex items-center gap-1.5 rounded-pill border border-line bg-bg/50 py-1 pl-3 pr-1.5 text-[13px] text-fg-2"
              >
                <Shield className="h-3.5 w-3.5 text-fg-3" aria-hidden />
                {app.name}
                {isDefault ? (
                  <span className="pr-1.5 text-xs text-fg-3">{t("controls.d3.protected.default")}</span>
                ) : (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void remove(app.name)}
                    aria-label={t("settings.removeProtectedApp")}
                    className="grid h-5 w-5 place-items-center rounded-pill text-fg-3 hover:bg-danger/10 hover:text-danger"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      </Card>
    </div>
  );
}
