import { useMemo, useState } from "react";
import { History as HistoryIcon, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Kpi } from "@/components/ui/kpi";
import { Segmented } from "@/components/ui/segmented";
import { SectionLabel } from "@/components/ui/section-label";
import { toast } from "@/hooks/use-toast";
import { useI18n } from "@/i18n";
import { undoSnapshot, useTimeline } from "@/features/history/useHistory";
import { startOfDay, type TimelineItem } from "@/features/history/timeline";
import { HistoryItem } from "./HistoryItem";

type Period = "today" | "week" | "month";
type Origin = "all" | "auto" | "you";

const DAY = 24 * 60 * 60;

function periodStart(period: Period): number {
  const today = startOfDay(Math.floor(Date.now() / 1000));
  if (period === "today") return today;
  return today - (period === "week" ? 6 : 29) * DAY;
}

/**
 * Histórico (board Tela-Historico): tudo o que o AnalystBlaze mudou, por
 * dia, com o que mudou e Desfazer por linha. Fonte: snapshots + auditoria
 * do agente (features/history).
 */
export function History() {
  const { t, locale } = useI18n();
  const timeline = useTimeline();
  const [period, setPeriod] = useState<Period>("today");
  const [origin, setOrigin] = useState<Origin>("all");
  const [undoingId, setUndoingId] = useState<string | null>(null);

  const visible = useMemo(() => {
    const from = periodStart(period);
    return timeline.items.filter((item) => {
      if (item.at < from) return false;
      if (origin === "auto") return item.source === "auto";
      if (origin === "you") return item.source === "you" || item.source === "web";
      return true;
    });
  }, [timeline.items, period, origin]);

  const days = useMemo(() => {
    const groups = new Map<number, TimelineItem[]>();
    for (const item of visible) {
      const day = startOfDay(item.at);
      groups.set(day, [...(groups.get(day) ?? []), item]);
    }
    return Array.from(groups.entries());
  }, [visible]);

  const weekFrom = periodStart("week");
  const week = timeline.items.filter((item) => item.at >= weekFrom);
  const activeNow = timeline.items.filter((item) => item.snapshotId && item.restoredAt === null).length;
  const undone = week.filter((item) => item.restoredAt !== null).length;

  const onUndo = async (snapshotId: string) => {
    setUndoingId(snapshotId);
    try {
      const report = await undoSnapshot(snapshotId);
      toast({
        title: report.failed_entries > 0 ? t("history.undoPartial") : t("history.undoDone"),
        description: report.messages.slice(0, 3).join("\n") || undefined,
        variant: report.failed_entries > 0 ? "destructive" : undefined,
      });
    } catch (error) {
      toast({ title: t("history.undoFailed"), description: String(error), variant: "destructive" });
    } finally {
      setUndoingId(null);
    }
  };

  const dayLabel = (day: number) => {
    const today = startOfDay(Math.floor(Date.now() / 1000));
    const date = new Date(day * 1000).toLocaleDateString(locale, { weekday: "long", day: "numeric", month: "long" });
    if (day === today) return `${t("history.today")} · ${date}`;
    if (day === today - DAY) return `${t("history.yesterday")} · ${date}`;
    return date;
  };

  return (
    <div className="flex flex-col gap-5">
      <header>
        <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("history.title")}</h2>
        <p className="mt-1 text-sm text-fg-3">{t("history.subtitle")}</p>
      </header>

      <div className="flex flex-wrap items-center gap-3">
        <Segmented<Period>
          variant="pill"
          aria-label={t("history.filters.period")}
          value={period}
          onChange={setPeriod}
          options={[
            { value: "today", label: t("history.filters.today") },
            { value: "week", label: t("history.filters.week") },
            { value: "month", label: t("history.filters.month") },
          ]}
        />
        <span aria-hidden className="h-5 w-px bg-line" />
        <Segmented<Origin>
          variant="pill"
          aria-label={t("history.filters.origin")}
          value={origin}
          onChange={setOrigin}
          options={[
            { value: "all", label: t("history.filters.all") },
            { value: "auto", label: t("history.filters.auto") },
            { value: "you", label: t("history.filters.you") },
          ]}
        />
      </div>

      <Card padding="md" className="grid grid-cols-2 gap-4 md:grid-cols-4">
        <Kpi label={t("history.kpi.week")} value={t("history.kpi.adjustments", { count: week.length })} />
        <Kpi label={t("history.kpi.active")} value={activeNow} />
        <Kpi label={t("history.kpi.undone")} value={undone} />
        <Kpi label={t("history.kpi.gain")} value={<span className="text-sm font-normal text-fg-3">{t("history.kpi.gainPending")}</span>} />
      </Card>

      {timeline.error && timeline.items.length === 0 ? (
        <EmptyState
          icon={<HistoryIcon />}
          title={t("history.errorTitle")}
          reason={timeline.error}
          action={
            <Button variant="outline" onClick={() => void timeline.refresh()}>
              <RefreshCw />
              {t("history.retry")}
            </Button>
          }
        />
      ) : days.length === 0 ? (
        <EmptyState
          icon={<HistoryIcon />}
          title={timeline.loading ? t("history.loading") : t("history.emptyTitle")}
          reason={timeline.loading ? null : t("history.emptyReason")}
        />
      ) : (
        <Card padding="md" className="flex flex-col gap-5">
          {days.map(([day, items]) => (
            <section key={day}>
              <SectionLabel className="border-b border-line-soft pb-2">{dayLabel(day)}</SectionLabel>
              <ul>
                {items.map((item) => (
                  <HistoryItem
                    key={item.id}
                    item={item}
                    onUndo={(id) => void onUndo(id)}
                    undoing={undoingId === item.snapshotId}
                  />
                ))}
              </ul>
            </section>
          ))}
        </Card>
      )}
    </div>
  );
}

/** "Voltar o PC como estava…" no canto do header: ainda sem tela (doc 15 §6). */
export function RestoreToDateButton() {
  const { t } = useI18n();
  return (
    <Button size="sm" variant="outline" disabledReason={t("history.restoreToDateSoon")}>
      {t("history.restoreToDate")}
    </Button>
  );
}
