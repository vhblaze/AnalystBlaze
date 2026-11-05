import { Fragment, type ReactNode } from "react";

/**
 * Header D3 (doc 15 §5): título da tela em mono à esquerda
 * ("DASHBOARD", "CONTROLES › AVANÇADO") e um slot à direita (busca, specs
 * do PC, "← Controles"). Sem status do agente, relógio ou avatar.
 */
export function Header({ title, right }: { title: string[]; right?: ReactNode }) {
  return (
    <header className="relative z-10 flex h-14 shrink-0 items-center justify-between gap-6 border-b border-line px-10">
      <h1 className="flex shrink-0 items-center gap-3 font-mono text-xs font-medium uppercase tracking-[0.32em] text-fg-2">
        {title.map((part, index) => (
          <Fragment key={`${index}:${part}`}>
            {index > 0 && <span aria-hidden className="text-fg-4">›</span>}
            <span>{part}</span>
          </Fragment>
        ))}
      </h1>
      {right && <div className="flex min-w-0 items-center justify-end gap-2">{right}</div>}
    </header>
  );
}
