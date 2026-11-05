import { History as HistoryIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { useI18n } from "@/i18n";

/**
 * Histórico. Etapa 1 do doc 15: a linha do tempo ainda mora em Controles
 * (snapshots e auditoria); esta tela diz isso e leva até lá. A linha do
 * tempo com Desfazer por item vem na etapa 3 (feat/dashboard-history).
 */
export function History({ onOpenControls }: { onOpenControls: () => void }) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <header>
        <h2 className="text-[28px] font-semibold tracking-tight text-fg">{t("history.title")}</h2>
        <p className="mt-1 text-sm text-fg-3">{t("history.subtitle")}</p>
      </header>
      <EmptyState
        icon={<HistoryIcon />}
        title={t("history.pendingTitle")}
        reason={t("history.pendingReason")}
        action={
          <Button variant="outline" onClick={onOpenControls}>
            {t("history.openControls")}
          </Button>
        }
      />
    </div>
  );
}
