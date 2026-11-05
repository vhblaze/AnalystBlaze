import { useId } from "react";
import markSvg from "@/brand/pulso-mark.svg?raw";
import markMonoSvg from "@/brand/pulso-mark-mono.svg?raw";
import { cn } from "@/lib/utils";

/*
 * Marca Pulso (doc 15 §7). O traço e as cores vêm dos arquivos de src/brand:
 * nada é redesenhado aqui, só lido. Embutido como <svg> (não <img>) para a
 * versão mono herdar currentColor.
 */

function attr(svg: string, element: string, name: string): string {
  const tag = svg.match(new RegExp(`<${element}\\b[^>]*>`))?.[0] ?? "";
  return tag.match(new RegExp(`\\s${name}="([^"]*)"`))?.[1] ?? "";
}

const MARK = {
  viewBox: attr(markSvg, "svg", "viewBox"),
  d: attr(markSvg, "path", "d"),
  strokeWidth: attr(markSvg, "path", "stroke-width"),
  stops: Array.from(markSvg.matchAll(/<stop\b[^>]*offset="([^"]+)"[^>]*stop-color="([^"]+)"/g)).map((match) => ({
    offset: match[1],
    color: match[2],
  })),
  gradient: {
    x1: attr(markSvg, "linearGradient", "x1"),
    y1: attr(markSvg, "linearGradient", "y1"),
    x2: attr(markSvg, "linearGradient", "x2"),
    y2: attr(markSvg, "linearGradient", "y2"),
  },
};

const MARK_MONO = {
  viewBox: attr(markMonoSvg, "svg", "viewBox"),
  d: attr(markMonoSvg, "path", "d"),
  strokeWidth: attr(markMonoSvg, "path", "stroke-width"),
};

type LogoMarkProps = {
  size?: number;
  /** `mono` usa currentColor e traço mais grosso (bandeja, 16 px). */
  variant?: "gradient" | "mono";
  /** Sem rótulo, a marca é decorativa (o nome já está ao lado). */
  title?: string;
  className?: string;
};

export function LogoMark({ size = 28, variant = "gradient", title, className }: LogoMarkProps) {
  const gradientId = useId();
  const mono = variant === "mono";
  const source = mono ? MARK_MONO : MARK;
  return (
    <svg
      viewBox={source.viewBox}
      width={size}
      height={size}
      className={cn("shrink-0", className)}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      {!mono && (
        <defs>
          <linearGradient id={gradientId} {...MARK.gradient}>
            {MARK.stops.map((stop) => (
              <stop key={stop.offset} offset={stop.offset} stopColor={stop.color} />
            ))}
          </linearGradient>
        </defs>
      )}
      <path
        d={source.d}
        fill="none"
        stroke={mono ? "currentColor" : `url(#${gradientId})`}
        strokeWidth={source.strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Marca + "AnalystBlaze", com "Blaze" em --accent-2 (doc 15 §5). */
export function Logo({ size = 28, className }: { size?: number; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2.5", className)}>
      <LogoMark size={size} />
      <span className="text-[15px] font-bold tracking-tight text-fg">
        Analyst<span className="text-accent-2">Blaze</span>
      </span>
    </span>
  );
}
