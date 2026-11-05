import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { useI18n } from "@/i18n";
import { Button } from "./button";

type ConfirmInlineProps = {
  /** Repete o que vai ser perdido ("Vai ser apagado de vez, sem volta · 9,4 GB"). */
  message: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** Ação sem volta: vermelho contornado. */
  danger?: boolean;
  busy?: boolean;
  className?: string;
};

/** Confirmação na própria linha (Disco, Winsock, quarentena), sem janela por cima. */
export function ConfirmInline({
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  danger = false,
  busy = false,
  className,
}: ConfirmInlineProps) {
  const { t } = useI18n();
  return (
    <div
      role="alertdialog"
      aria-live="polite"
      className={cn(
        "flex flex-wrap items-center gap-3 rounded-control border px-3 py-2",
        danger ? "border-danger-line bg-danger/5" : "border-line bg-surface-2",
        className,
      )}
    >
      <span className={cn("min-w-0 flex-1 text-[13px]", danger ? "text-danger" : "text-fg-2")}>{message}</span>
      <Button size="sm" variant="outline" onClick={onCancel} disabled={busy}>
        {t("common.cancel")}
      </Button>
      <Button size="sm" variant={danger ? "danger" : "outline"} onClick={onConfirm} disabled={busy} autoFocus>
        {confirmLabel}
      </Button>
    </div>
  );
}
