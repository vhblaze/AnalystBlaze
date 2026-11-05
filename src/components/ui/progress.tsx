import { cn } from "@/lib/utils";

type ProgressProps = {
  /** 0 a 100, sempre um valor medido. Sem valor, não use barra (doc 15 §4). */
  value: number;
  /** Texto lido por leitor de tela e mostrado ao lado ("calculando 30 de 45 pastas"). */
  label: string;
  showLabel?: boolean;
  tone?: "accent" | "ok" | "warn" | "danger";
  className?: string;
};

const fills = {
  accent: "bg-accent",
  ok: "bg-ok",
  warn: "bg-warn",
  danger: "bg-danger",
} as const;

/** Barra de progresso só com valor real: não existe modo indeterminado. */
export function Progress({ value, label, showLabel = false, tone = "accent", className }: ProgressProps) {
  const clamped = Number.isFinite(value) ? Math.max(0, Math.min(100, value)) : 0;
  return (
    <div className={cn("w-full", className)}>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(clamped)}
        className="h-1.5 w-full overflow-hidden rounded-pill bg-line"
      >
        <div
          className={cn("h-full rounded-pill transition-[width] duration-base ease-out", fills[tone])}
          style={{ width: `${clamped}%` }}
        />
      </div>
      {showLabel && <div className="mt-1.5 text-xs text-fg-3">{label}</div>}
    </div>
  );
}
