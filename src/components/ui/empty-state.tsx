import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

type EmptyStateProps = {
  /** O que não está aqui ("Ainda não medimos"). */
  title: ReactNode;
  /** Por quê, em português de gente ("A primeira medição leva 20 s"). */
  reason?: ReactNode;
  /** O que fazer para ter o dado ("Medir agora"). */
  action?: ReactNode;
  icon?: ReactNode;
  /** `inline` cabe dentro de um card; `block` ocupa uma área sozinho. */
  variant?: "inline" | "block";
  className?: string;
};

/** Substitui todo "--": diz o que falta, por quê e o que fazer (doc 15 §4). */
export function EmptyState({ title, reason, action, icon, variant = "block", className }: EmptyStateProps) {
  if (variant === "inline") {
    return (
      <div className={cn("flex flex-wrap items-center gap-x-3 gap-y-1 text-sm", className)}>
        <span className="text-fg-2">{title}</span>
        {reason && <span className="text-fg-3">{reason}</span>}
        {action}
      </div>
    );
  }
  return (
    <div
      className={cn(
        "flex flex-col items-start gap-2 rounded-card border border-dashed border-line px-5 py-6 text-sm",
        className,
      )}
    >
      {icon && <div className="text-fg-3 [&_svg]:size-5">{icon}</div>}
      <div className="font-semibold text-fg">{title}</div>
      {reason && <p className="max-w-prose text-fg-3">{reason}</p>}
      {action && <div className="mt-1">{action}</div>}
    </div>
  );
}
