import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { useI18n } from "@/i18n";
import { formatMegabytes, formatPercent } from "@/lib/format";
import { idleWeight, idleWeightKey, loadDismissed, saveDismissed } from "@/features/profile/useIdleWeight";
import type { IdleWeightItem } from "@/services/tauri/agent";

/**
 * "Pesa sem você usar" (etapa 2 do perfil): cada linha é um app de
 * inicialização ou serviço com custo medido agora e sem uso em primeiro
 * plano há 7 dias. As ações são as mesmas de Controles (com snapshot e
 * Desfazer); nada roda sozinho. Botões `outline`: o primário da tela é da
 * primeira sugestão.
 */
export function IdleWeightCard({
  onDelayStartupApp,
  onStopService,
}: {
  onDelayStartupApp: (name: string, location?: string | null) => Promise<unknown>;
  onStopService: (name: string) => Promise<unknown>;
}) {
  const { t } = useI18n();
  const { data } = idleWeight.use();
  const [dismissed, setDismissed] = useState<Record<string, number>>(() => loadDismissed());
  const [busy, setBusy] = useState<string | null>(null);

  const items = (data ?? []).filter((item) => !(idleWeightKey(item) in dismissed));
  if (items.length === 0) return null;

  const dismiss = (item: IdleWeightItem) => {
    const next = { ...dismissed, [idleWeightKey(item)]: Date.now() };
    saveDismissed(next);
    setDismissed(next);
  };

  const apply = async (item: IdleWeightItem) => {
    const key = idleWeightKey(item);
    setBusy(key);
    try {
      if (item.action === "delay_startup") await onDelayStartupApp(item.name, item.location);
      else await onStopService(item.name);
      await idleWeight.refresh().catch(() => undefined);
    } finally {
      setBusy(null);
    }
  };

  const cost = (item: IdleWeightItem) => {
    const parts: string[] = [];
    if (item.memoryMb > 0) parts.push(formatMegabytes(item.memoryMb));
    if (item.cpuPercent >= 0.5) parts.push(t("insightsLocal.idleWeight.cpu", { percent: formatPercent(item.cpuPercent) }));
    return parts.join(" · ");
  };

  const usage = (item: IdleWeightItem) => {
    if (item.kind === "service") {
      return item.lastForegroundDaysAgo == null
        ? t("insightsLocal.idleWeight.serviceNever")
        : t("insightsLocal.idleWeight.serviceDays", { days: item.lastForegroundDaysAgo });
    }
    return item.lastForegroundDaysAgo == null
      ? t("insightsLocal.idleWeight.neverSeen", { days: item.daysTracked })
      : t("insightsLocal.idleWeight.lastSeen", { days: item.lastForegroundDaysAgo });
  };

  return (
    <Card padding="lg">
      <h3 className="text-[18px] font-semibold tracking-tight text-fg">{t("insightsLocal.idleWeight.title")}</h3>
      <p className="mt-2 text-sm leading-relaxed text-fg-2">{t("insightsLocal.idleWeight.explanation")}</p>
      <ul className="mt-4 divide-y divide-line">
        {items.map((item) => {
          const key = idleWeightKey(item);
          return (
            <li key={key} className="grid grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_auto] items-center gap-4 py-3 text-sm">
              <div className="min-w-0">
                <p className="truncate font-medium text-fg">{item.displayName}</p>
                <p className="text-xs text-fg-3">
                  {t(`insightsLocal.idleWeight.kind.${item.kind}`)}
                  {cost(item) ? ` · ${cost(item)}` : ""}
                </p>
              </div>
              <p className="text-xs text-fg-3">{usage(item)}</p>
              <div className="flex items-center gap-3">
                <Button size="sm" variant="outline" disabled={busy === key} onClick={() => void apply(item)}>
                  {busy === key ? t("controls.d3.working") : t(`insightsLocal.idleWeight.action.${item.action}`)}
                </Button>
                <button type="button" onClick={() => dismiss(item)} className="text-xs text-fg-3 transition-colors hover:text-fg">
                  {t("insightsD3.notNow")}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      <p className="mt-3 text-xs text-fg-4">{t("insightsLocal.idleWeight.foot")}</p>
    </Card>
  );
}
