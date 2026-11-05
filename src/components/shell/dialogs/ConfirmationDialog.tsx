import { X } from "lucide-react";

export type ConfirmRequest = {
  id: string;
  title: string;
  description: string;
  risk: string;
  snapshot: boolean;
  remote?: boolean;
  timeoutMs?: number;
  resolve: (approved: boolean) => void;
};

export function ConfirmationDialog({
  request,
  onCancel,
  onConfirm,
}: {
  request: ConfirmRequest;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-bg/75 px-4 py-8">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={request.title}
        // Same fix as UpdateNotice: a long description (helper-health,
        // device-transfer, ...) used to push "Cancelar"/"Confirmar" off the
        // bottom of the screen with nothing to scroll and nothing to close -
        // this dialog fires far more often than the update one (every 30min
        // helper check, on every window focus), so it was the bigger source
        // of users getting stuck. Body scrolls on its own; the button row
        // stays outside that scroll region so it's never something you have
        // to scroll past a wall of text to reach.
        className="relative flex max-h-[85vh] w-full max-w-lg flex-col rounded-2xl border border-line bg-bg"
      >
        <button
          type="button"
          onClick={onCancel}
          aria-label="Fechar"
          className="absolute right-3 top-3 z-10 rounded-lg p-1.5 text-fg-3 transition hover:bg-white/5 hover:text-fg"
        >
          <X className="h-4 w-4" />
        </button>

        <div className="min-h-0 flex-1 overflow-y-auto p-6 pb-4 pr-10">
          <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-warn">
            {request.remote ? "pedido recebido da web" : "confirmação local"}
          </div>
          <h2 className="mt-2 text-xl font-semibold text-fg">{request.title}</h2>
          <p className="mt-2 text-sm leading-relaxed text-fg-3">{request.description}</p>
          <div className="mt-4 grid gap-2 sm:grid-cols-2">
            <div className="rounded-xl border border-line bg-surface/70 p-3">
              <span className="block font-mono text-[10px] uppercase tracking-widest text-fg-4">risco</span>
              <strong className="mt-1 block text-sm text-fg">{request.risk}</strong>
            </div>
            <div className="rounded-xl border border-line bg-surface/70 p-3">
              <span className="block font-mono text-[10px] uppercase tracking-widest text-fg-4">snapshot</span>
              <strong className="mt-1 block text-sm text-fg">{request.snapshot ? "obrigatório" : "não altera snapshot"}</strong>
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 border-t border-white/5 p-4">
          <button
            onClick={onCancel}
            className="rounded-xl border border-fg-4/60 px-4 py-2 text-sm font-medium text-fg-2 transition hover:border-fg-3/70"
          >
            Cancelar
          </button>
          <button
            onClick={onConfirm}
            className="rounded-xl border border-accent/50 bg-accent/10 px-4 py-2 text-sm font-semibold text-accent-soft transition hover:bg-accent/15"
          >
            Confirmar
          </button>
        </div>
      </section>
    </div>
  );
}
