import type { RemoteCommandConfirmationRequest } from "@/services/tauri/agent";

export function RemoteConfirmationNotice({
  request,
  count,
}: {
  request: RemoteCommandConfirmationRequest;
  count: number;
}) {
  return (
    <div className="mb-4 rounded-xl border border-warn/30 bg-warn/10 px-4 py-3 text-sm text-warn-soft">
      <div className="font-mono text-[10px] uppercase tracking-[0.24em] text-warn-soft/80">
        confirmacao pendente{count > 1 ? ` (${count})` : ""}
      </div>
      <div className="mt-1 font-semibold text-fg">{request.title || request.actionName}</div>
      <p className="mt-1 max-w-3xl text-xs leading-relaxed text-warn-soft/80">
        O dashboard pediu permissao para aplicar esta acao neste computador. A janela principal foi trazida para frente; confirme ou recuse no pop-up local.
      </p>
    </div>
  );
}
