import { useEffect, useMemo, useRef, useState } from "react";
import { Inbox, Search } from "lucide-react";
import type { useAuth } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { route, type Route } from "@/lib/routes";
import type { RunConfirmed } from "./useConfirmation";

export type CommandItem = {
  id: string;
  title: string;
  description?: string;
  keywords?: string[];
  /** Tipo do item: "view" (tela), "action" (ação), "auth" (conta). */
  hint?: string;
  disabled?: boolean;
  onSelect: () => void | Promise<void>;
};

/** Itens do Ctrl+K: as telas (incluindo as abas do Meu PC) e as ações rápidas. */
export function useCommandItems({
  auth,
  navigate,
  runConfirmed,
}: {
  auth: ReturnType<typeof useAuth>;
  navigate: (next: Route) => void;
  runConfirmed: RunConfirmed;
}) {
  const { t } = useI18n();
  return useMemo<CommandItem[]>(
    () => [
      {
        id: "view-dashboard",
        title: t("nav.dashboard"),
        description: t("palette.dashboard"),
        keywords: ["home", "inicio", "painel", "saude"],
        hint: "view",
        onSelect: () => navigate(route("dashboard")),
      },
      {
        id: "view-history",
        title: t("nav.history"),
        description: t("palette.history"),
        keywords: ["historico", "desfazer", "snapshots", "auditoria", "restaurar"],
        hint: "view",
        onSelect: () => navigate(route("history")),
      },
      {
        id: "view-insights",
        title: t("nav.insights"),
        description: t("palette.insights"),
        keywords: ["sugestoes", "recomendacoes", "insights"],
        hint: "view",
        onSelect: () => navigate(route("insights")),
      },
      {
        id: "view-controls",
        title: t("nav.controls"),
        description: t("palette.controls"),
        keywords: ["modo jogo", "limpeza", "inicializacao", "servicos", "energia", "componente", "helper"],
        hint: "view",
        onSelect: () => navigate(route("controls")),
      },
      {
        // Doc 14 U1: quem procura "Telemetria" tem que achar a aba Desempenho.
        id: "view-telemetry",
        title: `${t("nav.mypc")} › ${t("mypc.tabs.performance")}`,
        description: t("palette.performance"),
        keywords: ["telemetria", "tempo real", "metricas", "cpu", "gpu", "ram", "memoria", "temperatura"],
        hint: "view",
        onSelect: () => navigate(route("mypc", "performance")),
      },
      {
        id: "view-disk",
        title: `${t("nav.mypc")} › ${t("mypc.tabs.disk")}`,
        description: t("palette.disk"),
        keywords: ["disco", "espaco", "pastas", "armazenamento", "apagar"],
        hint: "view",
        onSelect: () => navigate(route("mypc", "disk")),
      },
      {
        id: "view-network",
        title: `${t("nav.mypc")} › ${t("mypc.tabs.network")}`,
        description: t("palette.network"),
        keywords: ["rede", "internet", "ping", "dns", "wifi", "latencia", "lag"],
        hint: "view",
        onSelect: () => navigate(route("mypc", "network")),
      },
      {
        id: "view-settings",
        title: t("nav.settings"),
        description: t("palette.settings"),
        keywords: ["conta", "idioma", "login", "tema", "preferencias"],
        hint: "view",
        onSelect: () => navigate(route("settings")),
      },
      {
        id: "action-start-agent",
        title: t("confirm.startAgent.title"),
        description: t("confirm.startAgent.body"),
        keywords: ["start", "telemetria", "conectar"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () => void auth.start(),
      },
      {
        id: "action-collect-sample",
        title: t("confirm.collectSample.title"),
        description: t("confirm.collectSample.body"),
        keywords: ["sample", "amostra", "metricas"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () => void auth.collectSample(),
      },
      {
        id: "action-pc-clean-fast",
        title: t("confirm.pcCleanFast.title"),
        description: t("confirm.pcCleanFast.body"),
        keywords: ["performance", "limpo", "rapido", "score"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.pcCleanFast.title"),
              description: t("confirm.pcCleanFast.body"),
              risk: "sensivel",
              snapshot: true,
            },
            auth.pcCleanFast,
          ),
      },
      {
        id: "action-game-mode",
        title: t("confirm.gameMode.title"),
        description: t("confirm.gameMode.body"),
        keywords: ["jogo", "game", "prioridade", "otimizacao"],
        hint: "action",
        disabled: auth.busy || !auth.status?.registered,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.gameMode.title"),
              description: t("confirm.gameMode.body"),
              risk: "sensivel",
              snapshot: true,
            },
            auth.activateGamePerformanceMode,
          ),
      },
      {
        id: "action-restore-game-mode",
        title: t("confirm.restoreGameMode.title"),
        description: t("confirm.restoreGameMode.body"),
        keywords: ["restaurar", "restore", "jogo"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.restoreGameMode.title"),
              description: t("confirm.restoreGameMode.body"),
              risk: "seguro",
              snapshot: false,
            },
            auth.restoreGameMode,
          ),
      },
      {
        id: "action-restore-snapshots",
        title: t("confirm.restoreAll.title"),
        description: t("confirm.restoreAll.body"),
        keywords: ["rollback", "restore", "snapshot"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.restoreAll.title"),
              description: t("confirm.restoreAll.body"),
              risk: "seguro",
              snapshot: false,
            },
            auth.restoreOptimizations,
          ),
      },
      {
        id: "action-deep-temp",
        title: t("confirm.deepTemp.title"),
        description: t("confirm.deepTemp.body"),
        keywords: ["temp", "limpeza", "cleanup"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.deepTemp.title"),
              description: t("confirm.deepTemp.body"),
              risk: "sensivel",
              snapshot: true,
            },
            auth.cleanTempDeep,
          ),
      },
      {
        id: "action-purge-cleanup",
        title: t("confirm.purgeQuarantine.title"),
        description: t("confirm.purgeQuarantine.body"),
        keywords: ["purge", "quarentena", "espaco", "disco"],
        hint: "action",
        disabled: auth.busy,
        onSelect: () =>
          void runConfirmed(
            {
              title: t("confirm.purgeQuarantine.title"),
              description: t("confirm.purgeQuarantine.body"),
              risk: "sensivel",
              snapshot: false,
            },
            auth.purgeCleanup,
          ),
      },
      {
        id: "action-login",
        title: t("confirm.login.title"),
        description: t("confirm.login.body"),
        keywords: ["login", "conta", "pareamento"],
        hint: "auth",
        disabled: auth.busy || Boolean(auth.status?.authenticated),
        onSelect: () => void auth.login(),
      },
    ],
    [auth, navigate, runConfirmed, t],
  );
}

function normalizeSearch(value: string) {
  return value
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .trim();
}

/** Campo "Buscar · Ctrl K" do header com a lista de resultados (saiu do TopBar). */
export function CommandPalette({ items }: { items: CommandItem[] }) {
  const { t } = useI18n();
  const track = useTelemetry("topbar");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const boxRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen(true);
        inputRef.current?.focus();
      }
      if (event.key === "Escape") {
        setOpen(false);
        inputRef.current?.blur();
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!boxRef.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, []);

  const results = useMemo(() => {
    const normalized = normalizeSearch(query);
    if (!normalized) return items.slice(0, 8);
    return items
      .filter((item) =>
        normalizeSearch([item.title, item.description, ...(item.keywords ?? [])].filter(Boolean).join(" ")).includes(
          normalized,
        ),
      )
      .slice(0, 8);
  }, [items, query]);

  const select = async (item: CommandItem) => {
    if (item.disabled) return;
    track("search_item_selected", { id: item.id });
    setQuery("");
    setOpen(false);
    inputRef.current?.blur();
    await item.onSelect();
  };

  const hintLabel = (hint?: string) => (hint ? t(`palette.hint.${hint}`) : null);

  return (
    <div ref={boxRef} className="relative">
      <label className="flex h-[30px] w-[244px] items-center gap-2 rounded-control border border-line bg-surface/60 px-2.5 text-fg-3 transition-colors duration-fast ease-out focus-within:border-accent/50">
        <Search className="h-3.5 w-3.5 shrink-0" aria-hidden />
        <input
          ref={inputRef}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && results[0]) {
              event.preventDefault();
              void select(results[0]);
            }
          }}
          placeholder={t("topbar.searchPlaceholder")}
          aria-label={t("topbar.searchPlaceholder")}
          className="min-w-0 flex-1 bg-transparent text-[13px] text-fg outline-none placeholder:text-fg-3"
        />
        <kbd className="shrink-0 rounded-[5px] border border-line px-1.5 py-px font-mono text-[10px] text-fg-3">
          {t("topbar.commandHint")}
        </kbd>
      </label>
      {open && (
        <div className="absolute right-0 top-10 z-40 w-[360px] overflow-hidden rounded-card border border-line bg-surface shadow-xl shadow-black/40">
          {results.length > 0 ? (
            <div className="max-h-96 overflow-y-auto p-1.5">
              {results.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  disabled={item.disabled}
                  onClick={() => void select(item)}
                  className="flex w-full items-start gap-3 rounded-control px-3 py-2.5 text-left transition-colors duration-fast hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-45"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium text-fg">{item.title}</span>
                    {item.description && (
                      <span className="mt-0.5 line-clamp-2 block text-xs text-fg-3">{item.description}</span>
                    )}
                  </span>
                  {hintLabel(item.hint) && (
                    <span className="label-mono shrink-0 pt-0.5 text-[10px]">{hintLabel(item.hint)}</span>
                  )}
                </button>
              ))}
            </div>
          ) : (
            <div className="flex items-center gap-2 px-4 py-4 text-sm text-fg-3">
              <Inbox className="h-4 w-4" />
              {t("topbar.searchEmpty")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
