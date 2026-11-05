import { useRef, type KeyboardEvent } from "react";
import { cn } from "@/lib/utils";

export type TabItem<T extends string> = {
  value: T;
  label: string;
  /** Ponto âmbar ("3 avisos do Windows"). O texto vira aria-label e dica. */
  attention?: string | null;
};

type TabsProps<T extends string> = {
  items: TabItem<T>[];
  active: T;
  onChange: (value: T) => void;
  "aria-label": string;
  className?: string;
};

/** Abas do Meu PC: sempre visíveis, sublinhado ciano na ativa (doc 15 §4). */
export function Tabs<T extends string>({ items, active, onChange, className, ...aria }: TabsProps<T>) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);

  const move = (event: KeyboardEvent, index: number) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const next = (index + (event.key === "ArrowRight" ? 1 : -1) + items.length) % items.length;
    refs.current[next]?.focus();
    onChange(items[next].value);
  };

  return (
    <div role="tablist" aria-label={aria["aria-label"]} className={cn("flex gap-7 border-b border-line", className)}>
      {items.map((item, index) => {
        const selected = item.value === active;
        return (
          <button
            key={item.value}
            ref={(element) => {
              refs.current[index] = element;
            }}
            type="button"
            role="tab"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(item.value)}
            onKeyDown={(event) => move(event, index)}
            title={item.attention ?? undefined}
            className={cn(
              "relative -mb-px flex h-10 items-center gap-1.5 border-b-2 text-sm transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
              selected ? "border-accent font-semibold text-fg" : "border-transparent text-fg-3 hover:text-fg",
            )}
          >
            {item.label}
            {item.attention && (
              <span role="img" aria-label={item.attention} className="h-1.5 w-1.5 rounded-pill bg-warn" />
            )}
          </button>
        );
      })}
    </div>
  );
}
