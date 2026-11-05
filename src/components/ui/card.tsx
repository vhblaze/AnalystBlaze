import type { HTMLAttributes, ReactNode } from "react";
import { cn } from "@/lib/utils";
import { SectionLabel } from "./section-label";

const paddings = {
  none: "",
  sm: "p-4",
  md: "p-5",
  lg: "p-6",
} as const;

type CardProps = HTMLAttributes<HTMLElement> & {
  /** Rótulo mono no topo ("ÚLTIMA OTIMIZAÇÃO"). */
  label?: ReactNode;
  /** Conteúdo à direita do rótulo (link, data, contador). */
  aside?: ReactNode;
  padding?: keyof typeof paddings;
};

/** Card D3: fundo --surface, borda --line, raio 16 (doc 15 §4). */
export function Card({ label, aside, padding = "md", className, children, ...props }: CardProps) {
  return (
    <section className={cn("rounded-card border border-line bg-surface", paddings[padding], className)} {...props}>
      {(label || aside) && (
        <div className="mb-3 flex items-baseline justify-between gap-3">
          {label ? <SectionLabel>{label}</SectionLabel> : <span />}
          {aside && <div className="text-xs text-fg-3">{aside}</div>}
        </div>
      )}
      {children}
    </section>
  );
}
