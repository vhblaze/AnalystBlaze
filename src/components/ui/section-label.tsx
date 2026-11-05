import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/** Rótulo de seção: mono 11 px, caixa alta, espaçamento 0,12em (doc 15 §3). */
export function SectionLabel({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("label-mono", className)} {...props} />;
}
