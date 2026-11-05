import { useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/i18n";
import type { TimelineItem } from "@/features/history/timeline";
import { cn } from "@/lib/utils";

function clock(unixSeconds: number, locale: string): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
}

/**
 * Uma linha do Histórico (board Tela-Historico): hora, o que mudou, quem
 * fez, o resultado medido e Desfazer. "o que mudou" abre a lista exata de
 * antes → depois que o snapshot guarda.
 */
export function HistoryItem({
  item,
  onUndo,
  undoing,
  compact = false,
}: {
  item: TimelineItem;
  onUndo?: (snapshotId: string) => void;
  undoing?: boolean;
  /** Versão curta para o "O que foi feito hoje" do Dashboard. */
  compact?: boolean;
}) {
  const { t, locale } = useI18n();
  const [open, setOpen] = useState(false);
  const title = t(item.titleKey) === item.titleKey ? t("history.actions.UNKNOWN") : t(item.titleKey);
  const restored = item.restoredAt !== null;
  const canUndo = Boolean(item.snapshotId && !restored && onUndo);
  const sourceLabel = item.source ? t(`history.source.${item.source}`) : null;

  return (
    <li className={cn("border-b border-line-soft last:border-b-0", compact ? "py-2.5" : "py-3.5")}>
      <div className="flex items-start gap-4">
        <time className="num w-12 shrink-0 pt-0.5 text-[13px] text-fg-3">{clock(item.at, locale)}</time>
        <div className="min-w-0 flex-1">
          <div className={cn("truncate font-medium text-fg", compact ? "text-sm" : "text-[15px]")}>{title}</div>
          {!compact && (
            <div className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-xs text-fg-3">
              {sourceLabel && <span>{sourceLabel}</span>}
              {sourceLabel && item.changes.length > 0 && <span aria-hidden>·</span>}
              {item.changes.length > 0 && (
                <button
                  type="button"
                  aria-expanded={open}
                  onClick={() => setOpen((value) => !value)}
                  className="text-accent-2 underline underline-offset-2 hover:text-accent"
                >
                  {t("history.whatChanged")}
                </button>
              )}
            </div>
          )}
        </div>
        {item.result && <span className="num shrink-0 pt-0.5 text-sm font-semibold text-ok">{item.result}</span>}
        <div className="shrink-0">
          {canUndo ? (
            <Button size="sm" variant="outline" disabled={undoing} onClick={() => onUndo?.(item.snapshotId as string)}>
              {undoing ? t("history.undoing") : t("history.undo")}
            </Button>
          ) : restored ? (
            <span className="text-xs text-fg-3">{t("history.restored")}</span>
          ) : compact && sourceLabel ? (
            <span className="text-xs text-fg-3">{sourceLabel}</span>
          ) : null}
        </div>
      </div>
      {open && !compact && (
        <div className="ml-16 mt-3 rounded-control border border-line bg-bg/40 px-4 py-3">
          <p className="mb-2 text-[13px] text-fg-2">{t("history.changesIntro", { count: item.changes.length })}</p>
          <ul className="flex flex-col gap-1.5">
            {item.changes.map((change, index) => (
              <li key={`${change.label}:${index}`} className="flex items-center gap-3 text-[13px]">
                <span className="min-w-0 flex-1 truncate text-fg">{change.label}</span>
                {change.before && <span className="shrink-0 text-fg-3">{change.before}</span>}
                {change.after && (
                  <span className="flex shrink-0 items-center gap-1 text-fg-2">
                    <ArrowRight className="h-3 w-3" aria-hidden />
                    {change.after}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </li>
  );
}
