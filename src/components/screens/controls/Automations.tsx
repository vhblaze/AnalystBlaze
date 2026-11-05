import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp, Lock } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { formatMinutes } from "@/lib/format";
import type { AgentStatus, LocalAiPolicy } from "@/services/tauri/agent";
import {
  saveAutomationPolicy,
  useAutomationPolicy,
  useWeeklyAutomationUsage,
} from "@/features/automations/useAutomationPolicy";
import { usePlan } from "@/features/plan/usePlan";

type FlagKey =
  | "auto_game_mode"
  | "auto_restore_game_mode"
  | "safe_temp_cleanup"
  | "optimize_power_plan"
  | "thermal_analysis_enabled"
  | "energy_estimation_enabled"
  | "auto_pc_clean"
  | "auto_best_dns"
  | "manage_startup_apps"
  | "reduce_background_processes"
  | "manage_services"
  | "allow_automatic_sensitive_actions";

type Automation = { key: FlagKey; proOnly?: boolean; detail?: "cleanupAges" | "idleEco" };

/** As 12 automações de antes (Configurações › modo adaptativo), por risco (doc 13 §4). */
const GROUPS: Array<{ id: "safe" | "more" | "sensitive"; items: Automation[] }> = [
  {
    id: "safe",
    items: [
      { key: "auto_game_mode", proOnly: true },
      { key: "auto_restore_game_mode" },
      { key: "safe_temp_cleanup", detail: "cleanupAges" },
      { key: "optimize_power_plan", detail: "idleEco" },
      { key: "thermal_analysis_enabled" },
      { key: "energy_estimation_enabled" },
    ],
  },
  {
    id: "more",
    items: [
      { key: "auto_pc_clean", proOnly: true },
      { key: "auto_best_dns" },
      { key: "manage_startup_apps" },
      { key: "reduce_background_processes" },
    ],
  },
  {
    id: "sensitive",
    items: [{ key: "manage_services" }, { key: "allow_automatic_sensitive_actions" }],
  },
];

const ALL_KEYS = GROUPS.flatMap((group) => group.items.map((item) => item.key));

function NumberRule({
  label,
  value,
  min,
  max,
  step,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix: string;
  onChange: (value: number) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-3 text-xs text-fg-2">
      <span>{label}</span>
      <span className="flex items-center gap-1.5">
        <input
          type="number"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(event) => {
            const next = Number(event.target.value);
            if (Number.isFinite(next)) onChange(Math.max(min, Math.min(max, next)));
          }}
          className="num h-7 w-20 rounded-[6px] border border-line bg-bg/60 px-2 text-right text-fg outline-none focus:border-accent/60"
        />
        <span className="w-6 text-fg-3">{suffix}</span>
      </span>
    </label>
  );
}

/**
 * Automações ativas (board Tela-Controles, coluna direita): o que o agente
 * pode fazer sozinho, com interruptor. Só regras: nada aprende nem adivinha.
 * No Starter, as que são do Pro ficam com cadeado e o motivo.
 */
export function Automations({ status }: { status: AgentStatus | null }) {
  const { t } = useI18n();
  const track = useTelemetry("controls_automations");
  const plan = usePlan(status);
  const { policy } = useAutomationPolicy();
  const weekly = useWeeklyAutomationUsage();
  const [openDetail, setOpenDetail] = useState<FlagKey | null>(null);
  const [saving, setSaving] = useState(false);

  const update = (patch: Partial<LocalAiPolicy>) => {
    if (!policy) return;
    setSaving(true);
    void saveAutomationPolicy({ ...policy, ...patch })
      .then((saved) => track("local_ai_policy_saved", { enabled: saved.enabled, maxRisk: saved.max_risk }))
      .catch(() => undefined)
      .finally(() => setSaving(false));
  };

  if (!policy) {
    return (
      <Card label={t("controls.d3.auto.label")} className="self-start">
        <p className="text-sm text-fg-3">{t("controls.d3.auto.loading")}</p>
      </Card>
    );
  }

  const isOn = (item: Automation) => Boolean(policy[item.key]) && !(item.proOnly && !plan.automaticGameMode);
  const onCount = ALL_KEYS.filter((key) => {
    const item = GROUPS.flatMap((group) => group.items).find((entry) => entry.key === key) as Automation;
    return isOn(item);
  }).length;

  const detail = (item: Automation): ReactNode => {
    if (item.detail === "cleanupAges") {
      return (
        <div className="flex flex-col gap-2">
          <NumberRule label={t("controls.d3.auto.rules.tempAge")} value={policy.cleanup_temp_min_age_minutes} min={5} max={10080} step={5} suffix="min" onChange={(cleanup_temp_min_age_minutes) => update({ cleanup_temp_min_age_minutes })} />
          <NumberRule label={t("controls.d3.auto.rules.cacheAge")} value={policy.cleanup_cache_min_age_minutes} min={10} max={10080} step={10} suffix="min" onChange={(cleanup_cache_min_age_minutes) => update({ cleanup_cache_min_age_minutes })} />
          <NumberRule label={t("controls.d3.auto.rules.systemAge")} value={policy.cleanup_system_min_age_minutes} min={60} max={43200} step={60} suffix="min" onChange={(cleanup_system_min_age_minutes) => update({ cleanup_system_min_age_minutes })} />
        </div>
      );
    }
    if (item.detail === "idleEco") {
      return (
        <NumberRule label={t("controls.d3.auto.rules.idleEco")} value={policy.adaptive_idle_eco_threshold_seconds} min={60} max={10800} step={30} suffix="s" onChange={(adaptive_idle_eco_threshold_seconds) => update({ adaptive_idle_eco_threshold_seconds })} />
      );
    }
    return null;
  };

  return (
    <Card
      label={t("controls.d3.auto.label")}
      aside={t("controls.d3.auto.count", { on: onCount, total: ALL_KEYS.length })}
      className="self-start"
    >
      <div className="flex items-center justify-between gap-4 border-b border-line-soft pb-3">
        <div>
          <div className="text-[15px] font-semibold text-fg">{t("controls.d3.auto.master")}</div>
          <div className="text-xs text-fg-3">{t("controls.d3.auto.masterDesc")}</div>
        </div>
        <Switch
          aria-label={t("controls.d3.auto.master")}
          checked={policy.enabled}
          disabled={saving}
          onCheckedChange={(enabled) => update({ enabled, agent_mode: enabled ? "automatic" : "manual" })}
        />
      </div>

      {!plan.automaticGameMode && (
        <p className="mt-3 flex items-start gap-2 text-xs text-fg-3">
          <Lock className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          {t("controls.d3.auto.starterNotice")}
          {weekly?.limitSeconds != null &&
            ` ${weekly.limitReached ? t("controls.d3.auto.weeklyUsedUp") : t("controls.d3.auto.weeklyLeft", { minutes: formatMinutes(weekly.remainingSeconds ?? 0) })}`}
        </p>
      )}

      {GROUPS.map((group) => (
        <section key={group.id} className="mt-4">
          <div className="text-xs font-semibold text-fg-2">{t(`controls.d3.auto.groups.${group.id}`)}</div>
          <ul className={policy.enabled ? "" : "opacity-60"}>
            {group.items.map((item) => {
              const locked = item.proOnly && !plan.automaticGameMode;
              const hasDetail = Boolean(item.detail);
              return (
                <li key={item.key} className="border-b border-line-soft py-2.5 last:border-b-0">
                  <div className="flex items-center gap-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5 text-sm font-medium text-fg">
                        {t(`controls.d3.auto.items.${item.key}.title`)}
                        {locked && <Lock className="h-3.5 w-3.5 text-fg-3" aria-label={t("controls.d3.auto.proOnly")} />}
                      </div>
                      <div className="text-xs text-fg-3">
                        {locked ? t("controls.d3.auto.proOnly") : t(`controls.d3.auto.items.${item.key}.desc`)}
                        {hasDetail && !locked && (
                          <button
                            type="button"
                            aria-expanded={openDetail === item.key}
                            onClick={() => setOpenDetail((current) => (current === item.key ? null : item.key))}
                            className="ml-1.5 inline-flex items-center gap-0.5 text-accent-2 hover:text-accent"
                          >
                            {t("controls.d3.auto.adjust")}
                            {openDetail === item.key ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
                          </button>
                        )}
                      </div>
                    </div>
                    <Switch
                      aria-label={t(`controls.d3.auto.items.${item.key}.title`)}
                      checked={isOn(item)}
                      disabled={locked || saving || !policy.enabled}
                      onCheckedChange={(value) =>
                        update(
                          item.key === "manage_services"
                            ? { manage_services: value, max_risk: value ? "sensitive" : policy.max_risk }
                            : ({ [item.key]: value } as Partial<LocalAiPolicy>),
                        )
                      }
                    />
                  </div>
                  {openDetail === item.key && <div className="mt-2 rounded-control border border-line bg-bg/40 p-3">{detail(item)}</div>}
                </li>
              );
            })}
          </ul>
        </section>
      ))}

      <div className="mt-4 flex items-center justify-between gap-3 border-t border-line-soft pt-3">
        <div>
          <div className="text-sm font-medium text-fg">{t("controls.d3.auto.maxRisk")}</div>
          <div className="text-xs text-fg-3">{t("controls.d3.auto.maxRiskDesc")}</div>
        </div>
        <Segmented<"safe" | "sensitive">
          size="sm"
          aria-label={t("controls.d3.auto.maxRisk")}
          value={policy.max_risk === "sensitive" ? "sensitive" : "safe"}
          onChange={(max_risk) => update({ max_risk })}
          options={[
            { value: "safe", label: t("controls.d3.auto.riskSafe") },
            { value: "sensitive", label: t("controls.d3.auto.riskSensitive") },
          ]}
        />
      </div>
    </Card>
  );
}
