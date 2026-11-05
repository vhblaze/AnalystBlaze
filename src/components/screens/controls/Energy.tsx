import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Segmented } from "@/components/ui/segmented";
import { useI18n } from "@/i18n";
import { formatMhzRange, formatPercent } from "@/lib/format";
import { useEnergyDiagnostics } from "@/features/health/useEnergyDiagnostics";
import type { ControlsActions, PowerPlan } from "./actions";
import { useActionRunner } from "./useActionRunner";

const PLAN_BY_ALIAS: Record<string, PowerPlan> = {
  high_performance: "high_performance",
  balanced: "balanced",
  power_saver: "power_saver",
};

export function powerPlanLabelKey(alias?: string | null): string {
  const plan = alias ? PLAN_BY_ALIAS[alias] : undefined;
  return plan ? `controls.d3.energy.plans.${plan}` : "controls.d3.energy.plans.other";
}

/**
 * Controles › Energia e visual (board Controles-Avancado, card Energia e
 * visual). O plano ativo vem do Windows; os efeitos visuais não têm leitura
 * do estado atual, então são dois botões e não um seletor que adivinha.
 */
export function Energy({ actions }: { actions: ControlsActions }) {
  const { t } = useI18n();
  const { energy, loading, refresh } = useEnergyDiagnostics();
  const { run, busyKey } = useActionRunner();
  const current = energy?.active_scheme_alias ? PLAN_BY_ALIAS[energy.active_scheme_alias] : undefined;
  const refreshEnergy = [() => refresh()];

  const rows: Array<[string, string]> = energy
    ? [
        [t("controls.d3.energy.source"), energy.power_source === "ac" ? t("controls.d3.energy.ac") : energy.power_source === "battery" ? t("controls.d3.energy.battery") : t("controls.d3.energy.unknown")],
        [t("controls.d3.energy.batteryLevel"), energy.battery_percent != null ? formatPercent(energy.battery_percent) : t("controls.d3.energy.noBattery")],
        [t("controls.d3.energy.saver"), energy.battery_saver_on == null ? t("controls.d3.energy.unknown") : energy.battery_saver_on ? t("controls.d3.energy.on") : t("controls.d3.energy.off")],
        [t("controls.d3.energy.cpuClock"), formatMhzRange(energy.cpu_current_clock_mhz, energy.cpu_max_clock_mhz, t("controls.d3.energy.unknown"))],
        [t("controls.d3.energy.recommended"), t(powerPlanLabelKey(energy.recommended_plan))],
      ]
    : [];

  return (
    <div className="flex max-w-3xl flex-col gap-5">
      <header>
        <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("controls.d3.energy.title")}</h2>
        <p className="mt-1 text-sm text-fg-3">{t("controls.d3.energy.subtitle")}</p>
      </header>

      <Card label={t("controls.d3.energy.planLabel")}>
        <Segmented<PowerPlan>
          aria-label={t("controls.d3.energy.planLabel")}
          value={current ?? ("" as PowerPlan)}
          onChange={(plan) =>
            void run(`plan:${plan}`, () => actions.onSetPowerPlan(plan), {
              done: t("controls.d3.energy.planChanged", { plan: t(`controls.d3.energy.plans.${plan}`) }),
              refresh: refreshEnergy,
            })
          }
          options={(["power_saver", "balanced", "high_performance"] as PowerPlan[]).map((plan) => ({
            value: plan,
            label: t(`controls.d3.energy.plans.${plan}`),
            disabled: busyKey !== null,
          }))}
        />
        {!current && energy && <p className="mt-2 text-xs text-fg-3">{t("controls.d3.energy.customPlan", { name: energy.active_scheme_name ?? "" })}</p>}
        {rows.length > 0 && (
          <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
            {rows.map(([label, value]) => (
              <div key={label} className="flex justify-between gap-3 border-b border-line-soft pb-2">
                <dt className="text-fg-3">{label}</dt>
                <dd className="num text-right text-fg">{value}</dd>
              </div>
            ))}
          </dl>
        )}
        {energy?.recommendations?.length ? (
          <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-fg-3">
            {energy.recommendations.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        ) : null}
        {!energy && <p className="mt-3 text-sm text-fg-3">{loading ? t("controls.d3.measuring") : t("controls.d3.energy.noReading")}</p>}
      </Card>

      <Card label={t("controls.d3.energy.visualLabel")}>
        <p className="text-sm text-fg-2">{t("controls.d3.energy.visualDesc")}</p>
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={busyKey !== null}
            onClick={() => void run("visual", actions.onApplyVisualPerformance, { done: t("controls.d3.energy.visualApplied") })}
          >
            {t("controls.d3.energy.visualPerformance")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busyKey !== null}
            onClick={() => void run("visual-restore", actions.onRestoreVisualPerformance, { done: t("controls.d3.energy.visualRestored") })}
          >
            {t("controls.d3.energy.visualDefault")}
          </Button>
        </div>
      </Card>
    </div>
  );
}
