import { useRef, type KeyboardEvent } from "react";
import { cn } from "@/lib/utils";

export type SegmentedOption<T extends string> = {
  value: T;
  label: string;
  disabled?: boolean;
};

type SegmentedProps<T extends string> = {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  "aria-label": string;
  size?: "md" | "sm";
  className?: string;
};

/** Seletor segmentado: Liga/Adia/Não, Tamanho/Nome, 1 min/10 min/1 hora. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  size = "md",
  className,
  ...aria
}: SegmentedProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const move = (event: KeyboardEvent, index: number) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const step = event.key === "ArrowRight" ? 1 : -1;
    for (let i = 1; i <= options.length; i += 1) {
      const next = (index + step * i + options.length) % options.length;
      if (!options[next].disabled) {
        refs.current[next]?.focus();
        onChange(options[next].value);
        return;
      }
    }
  };

  return (
    <div
      role="radiogroup"
      aria-label={aria["aria-label"]}
      className={cn("inline-flex items-center gap-0.5 rounded-control border border-line bg-bg/40 p-0.5", className)}
    >
      {options.map((option, index) => {
        const active = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={active ? 0 : -1}
            disabled={option.disabled}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => move(event, index)}
            className={cn(
              "rounded-[8px] font-medium transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-40",
              size === "md" ? "h-8 px-3.5 text-[13px]" : "h-7 px-2.5 text-xs",
              active ? "bg-accent/15 text-fg" : "text-fg-3 hover:text-fg",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
