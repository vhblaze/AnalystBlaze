import { useCallback, useState } from "react";
import { toast } from "@/hooks/use-toast";
import { useI18n } from "@/i18n";
import { auditLog, optimizationSnapshots } from "@/features/history/useHistory";
import { errorText } from "@/features/shared/agentResource";

type ResultLike = { success?: boolean; message?: string } | null | undefined;

/**
 * Roda uma ação de Controles: a confirmação já vem dentro dela; aqui fica o
 * "fazendo…" por botão, o aviso em toast (nunca janela por cima) e a releitura
 * do que mudou. Toda ação termina no Histórico, então ele é sempre relido.
 */
export function useActionRunner() {
  const { t } = useI18n();
  const [busyKey, setBusyKey] = useState<string | null>(null);

  const run = useCallback(
    async (
      key: string,
      action: () => Promise<unknown>,
      options: { done: string; refresh?: Array<() => Promise<unknown>> },
    ): Promise<boolean> => {
      setBusyKey(key);
      try {
        const result = await action();
        if (result === false) return false;
        const outcome = result as ResultLike;
        if (outcome && typeof outcome === "object" && outcome.success === false) {
          toast({ title: t("controls.d3.actionFailed"), description: outcome.message, variant: "destructive" });
          return false;
        }
        toast({ title: options.done });
        return true;
      } catch (error) {
        toast({ title: t("controls.d3.actionFailed"), description: errorText(error), variant: "destructive" });
        return false;
      } finally {
        await Promise.allSettled([
          auditLog.refresh(),
          optimizationSnapshots.refresh(),
          ...(options.refresh ?? []).map((refresh) => refresh()),
        ]);
        setBusyKey(null);
      }
    },
    [t],
  );

  return { run, busyKey, busy: busyKey !== null };
}
