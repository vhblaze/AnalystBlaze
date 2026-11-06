import { useState } from "react";
import { Briefcase, Clapperboard, Code2, Gamepad2, LayoutGrid } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import type { DeclaredProfile } from "@/services/tauri/agent";

const OPTIONS: ReadonlyArray<{ key: DeclaredProfile; icon: typeof Gamepad2 }> = [
  { key: "games", icon: Gamepad2 },
  { key: "work", icon: Briefcase },
  { key: "dev", icon: Code2 },
  { key: "creation", icon: Clapperboard },
  { key: "general", icon: LayoutGrid },
];

/**
 * Primeiro contato: "Para que você mais usa este PC?". Uma escolha vira o
 * perfil declarado na hora; a medição corre ao lado e pode sugerir a troca
 * depois. Só aparece uma vez; dá para mudar em Configurações.
 */
export function FirstContact({ onChoose }: { onChoose: (profile: DeclaredProfile) => Promise<void> }) {
  const { t } = useI18n();
  const [selected, setSelected] = useState<DeclaredProfile | null>(null);
  const [saving, setSaving] = useState(false);

  const confirm = async () => {
    if (!selected) return;
    setSaving(true);
    try {
      await onChoose(selected);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/85 px-4 py-8">
      <section role="dialog" aria-modal="true" aria-label={t("firstContact.title")} className="w-full max-w-2xl rounded-2xl border border-line bg-surface p-8">
        <p className="text-xs font-medium uppercase tracking-wide text-fg-3">{t("firstContact.kicker")}</p>
        <h2 className="mt-2 text-[26px] font-semibold tracking-tight text-fg">{t("firstContact.title")}</h2>
        <p className="mt-2 text-sm text-fg-2">{t("firstContact.body")}</p>
        <div className="mt-6 grid grid-cols-5 gap-3">
          {OPTIONS.map(({ key, icon: Icon }) => {
            const active = selected === key;
            return (
              <button
                key={key}
                type="button"
                onClick={() => setSelected(key)}
                aria-pressed={active}
                className={
                  "flex flex-col items-center gap-3 rounded-xl border px-3 py-5 text-sm transition-colors " +
                  (active ? "border-accent bg-accent/10 text-fg" : "border-line bg-surface-2 text-fg-2 hover:border-fg-4/60")
                }
              >
                <Icon className={"size-7 " + (active ? "text-accent" : "text-fg-3")} />
                <span className="font-medium">{t(`firstContact.options.${key}.label`)}</span>
                <span className="text-center text-xs text-fg-3">{t(`firstContact.options.${key}.hint`)}</span>
              </button>
            );
          })}
        </div>
        <p className="mt-4 text-xs text-fg-4">{t("firstContact.note")}</p>
        <div className="mt-6 flex justify-end">
          <Button variant="primary" disabled={!selected || saving} onClick={() => void confirm()}>
            {saving ? t("controls.d3.working") : t("firstContact.continue")}
          </Button>
        </div>
      </section>
    </div>
  );
}
