import {
  Activity,
  ChevronRight,
  CreditCard,
  ExternalLink,
  History,
  LayoutGrid,
  Lightbulb,
  LogIn,
  LogOut,
  Settings,
  Shield,
  UserCog,
  type LucideIcon,
} from "lucide-react";
import type { User } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";
import { Logo } from "@/design/icons";
import { route, SCREENS, type Route, type ScreenKey } from "@/lib/routes";
import { cn } from "@/lib/utils";
import { ProBadge } from "@/components/ui/pro-badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  isTauriRuntime,
  openAgentAccountSettings,
  openAgentBilling,
  type AgentStatus,
} from "@/services/tauri/agent";

const ICONS: Record<ScreenKey, LucideIcon> = {
  dashboard: LayoutGrid,
  history: History,
  insights: Lightbulb,
  controls: Shield,
  mypc: Activity,
  settings: Settings,
};

const openExternal = (url: string) => window.open(url, "_blank", "noopener,noreferrer");

async function openConfiguredExternal(action: "account" | "billing") {
  const url = action === "account" ? await openAgentAccountSettings() : await openAgentBilling();
  if (!isTauriRuntime()) openExternal(url);
}

function planLabel(plan: string) {
  const normalized = plan.trim().toLowerCase();
  if (!normalized || normalized === "free") return "Starter";
  return normalized.slice(0, 1).toUpperCase() + normalized.slice(1);
}

/** Sidebar D3 (doc 15 §5): 6 itens, cartão do usuário e pílula ASSINAR PRO. */
export function Sidebar({
  current,
  onNavigate,
  user,
  status,
  busy,
  onLogin,
  onLogout,
}: {
  current: Route;
  onNavigate: (next: Route) => void;
  user: User | null;
  status: AgentStatus | null;
  busy: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("sidebar");
  const isReady = Boolean(status?.authenticated && status.registered);
  const agentLabel = !status?.authenticated
    ? t("agent.status.waitingLogin")
    : isReady
      ? t("sidebar.connected")
      : t("agent.status.hardwarePending");

  return (
    <aside className="relative z-20 flex h-full w-60 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex items-center px-6 pb-5 pt-6">
        <Logo
          badge={
            // Estado do agente: o que antes era o "AGENT::ONLINE" do topo.
            <span
              role="img"
              aria-label={agentLabel}
              title={agentLabel}
              className={cn(
                "absolute -bottom-1 -right-1 h-2.5 w-2.5 rounded-pill ring-2 ring-surface",
                isReady ? "bg-ok" : "bg-warn",
              )}
            />
          }
        />
      </div>

      <nav aria-label={t("nav.section")} className="flex flex-col gap-0.5 px-4">
        {SCREENS.map(({ key, labelKey }) => {
          const Icon = ICONS[key];
          const active = current.screen === key;
          return (
            <button
              key={key}
              type="button"
              aria-current={active ? "page" : undefined}
              onClick={() => onNavigate(route(key))}
              className={cn(
                "relative flex h-9 items-center gap-3 rounded-control px-3.5 text-sm transition-colors duration-fast ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60",
                active
                  ? "bg-gradient-to-r from-accent/10 to-transparent font-semibold text-fg"
                  : "text-fg-2 hover:bg-surface-2 hover:text-fg",
              )}
            >
              {active && <span aria-hidden className="absolute inset-y-0 left-0 w-[3px] rounded-l-control bg-accent" />}
              <Icon className={cn("h-4 w-4 shrink-0", active ? "text-accent" : "text-fg-3")} />
              <span className="flex-1 text-left">{t(labelKey)}</span>
            </button>
          );
        })}
      </nav>

      <div className="mt-auto flex flex-col gap-2 p-4">
        {user ? (
          <div className="relative pt-3">
            {user.hasPaidPlan ? (
              <ProBadge label={planLabel(user.plan)} className="absolute -top-0.5 right-2 z-30" />
            ) : (
              <ProBadge
                label={t("sidebar.becomePro")}
                className="absolute -top-0.5 right-1.5 z-30"
                onClick={(event) => {
                  event.stopPropagation();
                  track("upgrade_to_pro_sidebar_badge_clicked");
                  void openConfiguredExternal("billing");
                }}
              />
            )}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="group flex min-h-[60px] w-full items-center gap-3 rounded-card border border-line bg-surface-2 p-3 pr-8 text-left transition-colors duration-fast ease-out hover:border-fg-4/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
                >
                  <span className="grid h-[34px] w-[34px] shrink-0 place-items-center rounded-pill border border-accent/50 bg-surface text-sm font-semibold text-fg">
                    {user.name.slice(0, 1).toUpperCase()}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-semibold text-fg">{user.name}</span>
                    <span className="block truncate text-[11px] text-fg-3">
                      {isReady ? t("sidebar.planLine", { plan: planLabel(user.plan) }) : agentLabel}
                    </span>
                  </span>
                  <ChevronRight className="h-3.5 w-3.5 text-fg-4 transition-transform duration-fast group-hover:translate-x-0.5 group-hover:text-fg-2" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent side="top" align="end" className="w-60 border-line bg-surface">
                <DropdownMenuLabel className="flex flex-col gap-0.5">
                  <span className="text-sm text-fg">{user.name}</span>
                  {user.email && <span className="truncate text-xs font-normal text-fg-3">{user.email}</span>}
                </DropdownMenuLabel>
                <DropdownMenuSeparator className="bg-line" />
                <DropdownMenuItem onClick={() => onNavigate(route("settings"))} className="gap-2">
                  <Settings className="h-4 w-4 text-fg-3" />
                  <span className="flex-1">{t("sidebar.viewAccount")}</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => {
                    track("external_account_clicked");
                    void openConfiguredExternal("account");
                  }}
                  className="gap-2"
                >
                  <UserCog className="h-4 w-4 text-fg-3" />
                  <span className="flex-1">{t("sidebar.manageAccount")}</span>
                  <ExternalLink className="h-3 w-3 text-fg-4" />
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => {
                    track(user.hasPaidPlan ? "external_billing_clicked" : "upgrade_to_pro_clicked");
                    void openConfiguredExternal("billing");
                  }}
                  className="gap-2"
                >
                  <CreditCard className="h-4 w-4 text-fg-3" />
                  <span className="flex-1">{user.hasPaidPlan ? t("sidebar.billing") : t("sidebar.becomePro")}</span>
                  <ExternalLink className="h-3 w-3 text-fg-4" />
                </DropdownMenuItem>
                <DropdownMenuSeparator className="bg-line" />
                <DropdownMenuItem onClick={() => void onLogout()} className="gap-2 text-danger focus:text-danger">
                  <LogOut className="h-4 w-4" />
                  {t("common.logout")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        ) : (
          <button
            type="button"
            disabled={busy}
            onClick={() => void onLogin()}
            className="group flex w-full items-center gap-3 rounded-card border border-accent/40 bg-surface-2 p-3 text-left transition-colors duration-fast ease-out hover:border-accent disabled:opacity-50"
          >
            <span className="grid h-[34px] w-[34px] place-items-center rounded-pill border border-accent/50 bg-surface">
              <LogIn className="h-4 w-4 text-accent" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block text-[13px] font-semibold text-fg">{t("sidebar.loginTitle")}</span>
              <span className="block text-[11px] text-fg-3">{t("sidebar.loginSubtitle")}</span>
            </span>
            <ChevronRight className="h-3.5 w-3.5 text-fg-3 transition-transform duration-fast group-hover:translate-x-0.5" />
          </button>
        )}
        <button
          type="button"
          onClick={() => onNavigate(route("settings"))}
          title={t("sidebar.viewAccount")}
          className="num mt-1 text-center font-mono text-[10px] text-fg-4 transition-colors duration-fast hover:text-fg-2"
        >
          {t("app.versionLine")}
        </button>
      </div>
    </aside>
  );
}
