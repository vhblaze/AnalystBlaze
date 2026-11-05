import type { MouseEvent } from "react";
import { cn } from "@/lib/utils";

type ProBadgeProps = {
  /** "ASSINAR PRO", "MANTER PRO" (teste grátis, doc 14 U8) ou o nome do plano pago. */
  label: string;
  /** Sem onClick vira um selo (plano já pago). */
  onClick?: (event: MouseEvent<HTMLButtonElement>) => void;
  className?: string;
};

const pill =
  "rotate-6 rounded-pill border border-accent-soft/50 bg-gradient-to-r from-accent to-violet px-2.5 py-1 font-mono text-[9px] font-bold uppercase tracking-widest text-accent-ink shadow-[0_0_16px_rgb(var(--accent-rgb)/0.55)]";

/**
 * A pílula inclinada sobre o cartão do usuário (degradê ciano -> violeta,
 * 6°, brilho), mantida a pedido do dono do produto (doc 13 §2.4). É o único
 * elemento do app que conserva brilho.
 */
export function ProBadge({ label, onClick, className }: ProBadgeProps) {
  if (!onClick) {
    return <span className={cn(pill, "pointer-events-none", className)}>{label}</span>;
  }
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        pill,
        "transition-transform duration-fast ease-out hover:rotate-3 hover:scale-105 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
        className,
      )}
    >
      {label}
    </button>
  );
}
