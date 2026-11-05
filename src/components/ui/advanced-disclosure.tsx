import { useId, useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { cn } from "@/lib/utils";
import { useI18n } from "@/i18n";

type AdvancedDisclosureProps = {
  /** Uma linha do que tem dentro ("Adaptadores, reparos rápidos, DNS manual e TCP"). */
  summary: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  className?: string;
};

/** "Avançado · Abrir ▾": abre dentro da própria aba, nunca em outra página (doc 15 §4). */
export function AdvancedDisclosure({ summary, children, defaultOpen = false, className }: AdvancedDisclosureProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(defaultOpen);
  const contentId = useId();

  return (
    <section className={cn("rounded-card border border-line bg-surface", className)}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={contentId}
        onClick={() => setOpen((value) => !value)}
        className="flex min-h-12 w-full items-center gap-3 rounded-card px-5 py-3 text-left transition-colors duration-fast ease-out hover:bg-surface-2/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
      >
        <span className="font-semibold text-fg">{t("ui.advanced.title")}</span>
        <span className="min-w-0 flex-1 truncate text-[13px] text-fg-3">{summary}</span>
        <span className="flex shrink-0 items-center gap-1 text-[13px] font-medium text-accent-2">
          {open ? t("ui.advanced.close") : t("ui.advanced.open")}
          {open ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
        </span>
      </button>
      {open && (
        <div id={contentId} className="border-t border-line px-5 py-4">
          {children}
        </div>
      )}
    </section>
  );
}
