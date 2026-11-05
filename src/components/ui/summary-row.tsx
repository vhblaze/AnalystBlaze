import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type SummaryRowProps = {
  title: ReactNode;
  /** Uma linha que responde "vale abrir?". */
  summary?: ReactNode;
  /** Estado curto à direita ("Equilibrado", "4 apps", <StatusPill/>). */
  state?: ReactNode;
  /** Pinta o resumo em âmbar quando o grupo pede atenção. */
  warn?: boolean;
  /** Normalmente um <Button size="sm">Abrir</Button> ou um <Switch/>. */
  action?: ReactNode;
  className?: string;
};

/** Linha de grupo em Controles e de item em Avançado (doc 15 §4). */
export function SummaryRow({ title, summary, state, warn, action, className }: SummaryRowProps) {
  return (
    <div className={cn("flex items-center gap-4 border-b border-line-soft py-3.5 last:border-b-0", className)}>
      <div className="min-w-0 flex-1">
        <div className="text-[15px] font-semibold leading-snug text-fg">{title}</div>
        {summary && (
          <div className={cn("mt-0.5 text-[13px] leading-snug", warn ? "text-warn" : "text-fg-3")}>{summary}</div>
        )}
      </div>
      {state && <div className="shrink-0 text-xs text-fg-3">{state}</div>}
      {action && <div className="shrink-0">{action}</div>}
    </div>
  );
}
