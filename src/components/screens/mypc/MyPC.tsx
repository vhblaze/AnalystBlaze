import type { ReactNode } from "react";
import { Tabs } from "@/components/ui/tabs";
import { useI18n } from "@/i18n";
import { MYPC_TABS, type MyPcTab } from "@/lib/routes";

/**
 * Meu PC: abas fixas Desempenho · Disco · Rede · Saúde do Windows (doc 13 §3).
 * Etapa 1: cada aba ainda mostra a tela atual (Telemetria, Disco, Rede);
 * o conteúdo novo de cada aba vem na etapa 5 (feat/mypc).
 */
export function MyPC({
  tab,
  onTabChange,
  children,
}: {
  tab: MyPcTab;
  onTabChange: (tab: MyPcTab) => void;
  children: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-6">
      <div data-tour="mypc-tabs">
        <Tabs
          aria-label={t("nav.mypc")}
          items={MYPC_TABS.map((item) => ({ value: item.key, label: t(item.labelKey) }))}
          active={tab}
          onChange={onTabChange}
        />
      </div>
      <div role="tabpanel" aria-label={t(MYPC_TABS.find((item) => item.key === tab)?.labelKey ?? "nav.mypc")}>
        {children}
      </div>
    </div>
  );
}
