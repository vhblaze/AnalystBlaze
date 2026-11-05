import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { useI18n } from "@/i18n";
import { displayProcessName, formatPercent } from "@/lib/format";
import { formatHours, sortedShares, usageProfile } from "@/features/profile/useUsageProfile";

/**
 * "Perfil deste PC": o que o AnalystBlaze mediu de uso nos últimos 7 dias,
 * com o porquê. Só descreve; nenhuma regra muda por causa dele nesta etapa.
 */
export function UsageProfileCard() {
  const { t } = useI18n();
  const { data, error } = usageProfile.use();

  if (error) {
    return (
      <Card label={t("dashboard.d3.usageProfile.label")}>
        <EmptyState title={t("dashboard.d3.usageProfile.collectingTitle")} reason={error} />
      </Card>
    );
  }
  if (!data) return null;

  if (data.profile === "collecting") {
    return (
      <Card label={t("dashboard.d3.usageProfile.label")}>
        <EmptyState
          title={t("dashboard.d3.usageProfile.collectingTitle")}
          reason={t("dashboard.d3.usageProfile.collectingReason")}
        />
      </Card>
    );
  }

  const shares = sortedShares(data.shares);
  const mainShare = shares[0]?.[1] ?? 0;
  const and = t("dashboard.d3.usageProfile.and");
  const reasons = data.reasons
    .filter((reason) => reason !== "peak_hours")
    .map((reason) => t(`dashboard.d3.usageProfile.reasons.${reason}`, { share: formatPercent(mainShare * 100) }));

  return (
    <Card
      label={t("dashboard.d3.usageProfile.label")}
      aside={t("dashboard.d3.usageProfile.measured", { days: data.daysObserved, window: data.windowDays })}
    >
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-8">
        <div>
          <p className="text-[26px] font-semibold tracking-tight text-fg">
            {t(`dashboard.d3.usageProfile.names.${data.profile}`)}
          </p>
          {reasons.length > 0 && <p className="mt-1 text-sm text-fg-2">{reasons.join(" · ")}</p>}
          <ul className="mt-4 grid gap-2">
            {shares.slice(0, 5).map(([category, share]) => (
              <li key={category} className="grid grid-cols-[140px_minmax(0,1fr)_48px] items-center gap-3 text-sm text-fg-2">
                <span>{t(`dashboard.d3.usageProfile.categories.${category}`)}</span>
                <span className="h-2 overflow-hidden rounded-full bg-surface-2">
                  <span className="block h-full rounded-full bg-accent" style={{ width: `${Math.round(share * 100)}%` }} />
                </span>
                <span className="text-right tabular-nums text-fg-3">{formatPercent(share * 100)}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="text-sm text-fg-2">
          <p className="text-xs font-medium uppercase tracking-wide text-fg-3">{t("dashboard.d3.usageProfile.topApps")}</p>
          <ul className="mt-2 grid gap-1.5">
            {data.topApps.slice(0, 4).map((app) => (
              <li key={app.name} className="flex justify-between gap-3">
                <span className="truncate">{displayProcessName(app.name.replace(/\.exe$/i, ""))}</span>
                <span className="tabular-nums text-fg-3">{formatPercent(app.share * 100)}</span>
              </li>
            ))}
          </ul>
          {data.peakHours.length > 0 && (
            <p className="mt-4 text-xs text-fg-3">{t("dashboard.d3.usageProfile.peakHours", { hours: formatHours(data.peakHours, and) })}</p>
          )}
          {data.idleHours.length > 0 && (
            <p className="mt-1 text-xs text-fg-3">{t("dashboard.d3.usageProfile.idleHours", { hours: formatHours(data.idleHours.slice(0, 3), and) })}</p>
          )}
          <p className="mt-3 text-xs text-fg-4">{t("dashboard.d3.usageProfile.how", { window: data.windowDays })}</p>
        </div>
      </div>
    </Card>
  );
}
