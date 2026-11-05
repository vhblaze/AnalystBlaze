import { useMemo, useState } from "react";
import { CircleCheck } from "lucide-react";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { toast } from "@/hooks/use-toast";
import { useI18n } from "@/i18n";
import { undoSnapshot, useTimeline } from "@/features/history/useHistory";
import { startOfDay } from "@/features/history/timeline";
import { HistoryItem } from "@/components/screens/history/HistoryItem";

const MAX_ROWS = 5;

/** "O que foi feito hoje": a mesma linha do tempo do Histórico, só o dia, com Desfazer. */
export function DoneToday({ onOpenHistory }: { onOpenHistory: () => void }) {
  const { t } = useI18n();
  const timeline = useTimeline();
  const [undoingId, setUndoingId] = useState<string | null>(null);
  const today = useMemo(() => {
    const from = startOfDay(Math.floor(Date.now() / 1000));
    return timeline.items.filter((item) => item.at >= from);
  }, [timeline.items]);

  const onUndo = async (snapshotId: string) => {
    setUndoingId(snapshotId);
    try {
      const report = await undoSnapshot(snapshotId);
      toast({ title: report.failed_entries > 0 ? t("history.undoPartial") : t("history.undoDone") });
    } catch (error) {
      toast({ title: t("history.undoFailed"), description: String(error), variant: "destructive" });
    } finally {
      setUndoingId(null);
    }
  };

  return (
    <Card
      label={t("dashboard.d3.doneToday")}
      aside={
        <button type="button" onClick={onOpenHistory} className="text-accent-2 underline underline-offset-4 hover:text-accent">
          {t("dashboard.d3.fullHistory")}
        </button>
      }
      className="flex min-h-[248px] flex-col"
    >
      {today.length === 0 ? (
        <EmptyState
          className="flex-1 border-none p-0"
          title={timeline.loading ? t("history.loading") : t("dashboard.d3.nothingToday")}
          reason={timeline.loading ? null : t("dashboard.d3.nothingTodayReason")}
        />
      ) : (
        <ul className="flex-1">
          {today.slice(0, MAX_ROWS).map((item) => (
            <HistoryItem
              key={item.id}
              compact
              item={item}
              onUndo={(id) => void onUndo(id)}
              undoing={undoingId === item.snapshotId}
            />
          ))}
        </ul>
      )}
      <p className="mt-4 flex items-center gap-2 text-[13px] text-fg-2">
        <CircleCheck className="h-4 w-4 text-ok" aria-hidden />
        {t("dashboard.d3.reversible")}
      </p>
    </Card>
  );
}
