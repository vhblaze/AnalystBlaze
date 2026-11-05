import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

const tones = {
  default: "text-fg",
  warn: "text-warn",
  ok: "text-ok",
  danger: "text-danger",
} as const;

type KpiProps = {
  label: ReactNode;
  /** Já formatado por lib/format. Sem valor, a tela usa <EmptyState variant="inline"/>. */
  value: ReactNode;
  /** Complemento menor ao lado ("4,9 GHz", "/ 32 GB", "livres"). */
  hint?: ReactNode;
  /** O número que pede atenção fica em âmbar (doc 11 §0). */
  tone?: keyof typeof tones;
  className?: string;
};

/** Um número da faixa de 4 do Dashboard e do Histórico. */
export function Kpi({ label, value, hint, tone = "default", className }: KpiProps) {
  return (
    <div className={cn("min-w-0", className)}>
      <div className="truncate text-xs text-fg-3">{label}</div>
      <div className="mt-1 flex items-baseline gap-1.5">
        <span className={cn("num text-xl font-semibold leading-none", tones[tone])}>{value}</span>
        {hint && <span className="num truncate text-xs text-fg-3">{hint}</span>}
      </div>
    </div>
  );
}
