import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

const tones = {
  ok: "border-ok/40 text-ok",
  warn: "border-warn/50 text-warn",
  muted: "border-line text-fg-3",
} as const;

type StatusPillProps = {
  tone?: keyof typeof tones;
  /** Ícone lucide opcional (Shield para "app protegido", Lock para "do Windows"). */
  icon?: ReactNode;
  children: ReactNode;
  className?: string;
};

/** Selo curto: "ativo", "desconectado", "app protegido", "do Windows", "sugerido tirar". */
export function StatusPill({ tone = "muted", icon, children, className }: StatusPillProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 whitespace-nowrap rounded-pill border px-2 py-0.5 text-[11px] font-medium leading-4 [&_svg]:size-3",
        tones[tone],
        className,
      )}
    >
      {icon}
      {children}
    </span>
  );
}
