import { Activity, ChevronRight, CreditCard, ExternalLink, LayoutDashboard, LogIn, LogOut, Settings, ShieldCheck, Sparkles, UserCog } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ViewKey } from "./AppShell";
import type { User } from "@/hooks/useAuth";
import type { AgentStatus } from "@/services/tauri/agent";
import { useI18n } from "@/i18n";
import { useTelemetry } from "@/hooks/useTelemetry";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const BILLING_URL = import.meta.env.VITE_ANALYSTBLAZE_BILLING_URL ?? "https://analystblaze.app/billing";
const ACCOUNT_URL = import.meta.env.VITE_ANALYSTBLAZE_ACCOUNT_URL ?? "https://analystblaze.app/account";

const items: { key: ViewKey; labelKey: string; icon: typeof Activity; hint: string }[] = [
  { key: "dashboard", labelKey: "nav.dashboard", icon: LayoutDashboard, hint: "01" },
  { key: "telemetry", labelKey: "nav.telemetry", icon: Activity, hint: "02" },
  { key: "insights", labelKey: "nav.insights", icon: Sparkles, hint: "03" },
  { key: "controls", labelKey: "nav.controls", icon: ShieldCheck, hint: "04" },
  { key: "settings", labelKey: "nav.settings", icon: Settings, hint: "05" },
];

const planStyles: Record<string, string> = {
  starter: "border-cyan-200/50 bg-gradient-to-r from-cyan-300 to-violet-300 text-slate-950 shadow-[0_0_18px_hsl(187_100%_60%/0.85)]",
  free: "border-cyan-200/50 bg-gradient-to-r from-cyan-300 to-violet-300 text-slate-950 shadow-[0_0_18px_hsl(187_100%_60%/0.85)]",
  pro: "border-cyan-200/50 bg-gradient-to-r from-cyan-300 to-violet-300 text-slate-950 shadow-[0_0_18px_hsl(187_100%_60%/0.85)]",
  ultra: "border-fuchsia-200/50 bg-gradient-to-r from-amber-300 via-fuchsia-300 to-violet-400 text-slate-950 shadow-[0_0_18px_hsl(290_100%_65%/0.8)]",
};

const openExternal = (url: string) => window.open(url, "_blank", "noopener,noreferrer");

export function Sidebar({
  view,
  onChange,
  user,
  status,
  busy,
  onLogin,
  onLogout,
}: {
  view: ViewKey;
  onChange: (v: ViewKey) => void;
  user: User | null;
  status: AgentStatus | null;
  busy: boolean;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
}) {
  const { t } = useI18n();
  const track = useTelemetry("sidebar");
  const isReady = Boolean(status?.authenticated && status.registered);
  const accountStatus = isReady ? t("sidebar.connected") : t("agent.status.hardwarePending");

  return (
    <aside className="relative z-20 flex h-full w-64 shrink-0 flex-col border-r border-cyan-500/10 bg-gradient-to-b from-slate-950/90 via-slate-950/70 to-slate-950/90 backdrop-blur-xl">
      <div className="flex items-center gap-3 px-6 py-6">
        <Logo />
        <div className="leading-tight">
          <div className="text-[15px] font-semibold tracking-tight text-slate-50">
            Analyst<span className="text-gradient-cyber">Blaze</span>
          </div>
          <div className="mt-0.5 text-[9px] font-mono uppercase tracking-[0.3em] text-cyan-400/60">
            {t("sidebar.subtitle")}
          </div>
        </div>
      </div>

      <div className="px-6">
        <div className="h-px bg-gradient-to-r from-transparent via-cyan-500/30 to-transparent" />
      </div>

      <nav className="flex flex-col gap-1.5 p-4 mt-4">
        <div className="px-3 pb-2 text-[10px] font-mono uppercase tracking-[0.25em] text-slate-600">
          {t("nav.section")}
        </div>
        {items.map(({ key, labelKey, icon: Icon, hint }) => {
          const active = view === key;
          return (
            <button
              key={key}
              onClick={() => onChange(key)}
              className={cn(
                "group relative flex items-center gap-3 overflow-hidden rounded-xl px-3.5 py-2.5 text-sm transition-all duration-300",
                active
                  ? "bg-gradient-to-r from-cyan-500/15 via-cyan-500/5 to-transparent text-cyan-200"
                  : "text-slate-400 hover:bg-slate-800/40 hover:text-slate-100"
              )}
            >
              <span
                className={cn(
                  "absolute left-0 top-1/2 h-7 -translate-y-1/2 rounded-r-full bg-gradient-to-b from-cyan-300 to-violet-400 transition-all",
                  active ? "w-[3px] opacity-100 shadow-[0_0_12px_hsl(187_100%_60%/0.8)]" : "w-0 opacity-0"
                )}
              />
              <Icon className={cn("h-4 w-4 shrink-0 transition-colors", active && "text-cyan-300")} />
              <span className="flex-1 text-left font-medium">{t(labelKey)}</span>
              <span className="font-mono text-[10px] text-slate-600">{hint}</span>
            </button>
          );
        })}
      </nav>

      <div className="mt-auto flex flex-col gap-2 p-4">
        {user ? (
          <div className="relative pt-3">
            {user.hasPaidPlan ? (
              <span className={cn(
                "pointer-events-none absolute -top-0.5 right-2 z-30 rotate-12 rounded-full border px-2.5 py-1 font-mono text-[9px] font-black uppercase tracking-widest",
                planStyles[user.plan] ?? planStyles.pro,
              )}>
                {planLabel(user.plan)}
              </span>
            ) : (
              <button
                onClick={(event) => {
                  event.stopPropagation();
                  track("upgrade_to_pro_sidebar_badge_clicked");
                  openExternal(BILLING_URL);
                }}
                className="absolute -top-0.5 right-1.5 z-30 rotate-6 rounded-full border border-cyan-200/50 bg-gradient-to-r from-cyan-300 to-violet-300 px-2.5 py-1 font-mono text-[9px] font-black uppercase tracking-widest text-slate-950 shadow-[0_0_18px_hsl(187_100%_60%/0.85)] transition-transform hover:rotate-3 hover:scale-105"
              >
                {t("sidebar.becomePro")}
              </button>
            )}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button className="group relative flex min-h-[76px] w-full items-center gap-3 rounded-xl border border-cyan-500/10 bg-slate-900/50 p-3 pr-8 text-left transition-all hover:border-cyan-400/40 hover:bg-slate-900/80">
                  <div className="grid h-11 w-11 place-items-center rounded-full border border-cyan-400/40 bg-gradient-to-br from-cyan-500/30 to-violet-500/20 text-sm font-semibold text-cyan-100">
                    {user.name.slice(0, 1).toUpperCase()}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-semibold text-slate-100">{user.name}</div>
                    <div className="font-mono text-[10px] text-slate-500">
                      {accountStatus}
                    </div>
                  </div>
                  <ChevronRight className="h-3.5 w-3.5 text-slate-600 transition-transform group-hover:translate-x-0.5 group-hover:text-cyan-300" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                side="top"
                align="end"
                className="w-60 border-cyan-500/20 bg-slate-950/95 backdrop-blur-xl"
              >
                <DropdownMenuLabel className="flex flex-col gap-0.5">
                  <span className="font-mono text-[10px] font-normal uppercase tracking-widest text-cyan-300">
                    {t("sidebar.currentPlan")}: {planLabel(user.plan)}
                  </span>
                  <span className="text-sm text-slate-100">{user.name}</span>
                  {user.email && <span className="truncate text-xs font-normal text-slate-500">{user.email}</span>}
                  <span className="font-mono text-[10px] font-normal text-slate-500">
                    {t("sidebar.session")} - {user.sessionId.slice(0, 6)}...{user.sessionId.slice(-4)}
                  </span>
                </DropdownMenuLabel>
                <DropdownMenuSeparator className="bg-cyan-500/10" />
                <DropdownMenuItem
                  onClick={() => {
                    track("external_account_clicked");
                    openExternal(ACCOUNT_URL);
                  }}
                  className="gap-2 focus:bg-cyan-500/10 focus:text-cyan-100"
                >
                  <UserCog className="h-4 w-4 text-cyan-300" />
                  <span className="flex-1">{t("sidebar.manageAccount")}</span>
                  <ExternalLink className="h-3 w-3 text-slate-500" />
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => {
                    track(user.hasPaidPlan ? "external_billing_clicked" : "upgrade_to_pro_clicked");
                    openExternal(BILLING_URL);
                  }}
                  className="gap-2 focus:bg-cyan-500/10 focus:text-cyan-100"
                >
                  <CreditCard className="h-4 w-4 text-cyan-300" />
                  <span className="flex-1">{user.hasPaidPlan ? t("sidebar.billing") : t("sidebar.becomePro")}</span>
                  <ExternalLink className="h-3 w-3 text-slate-500" />
                </DropdownMenuItem>
                <DropdownMenuSeparator className="bg-cyan-500/10" />
                <DropdownMenuItem
                  onClick={() => void onLogout()}
                  className="gap-2 text-rose-300 focus:bg-rose-500/10 focus:text-rose-200"
                >
                  <LogOut className="h-4 w-4" />
                  {t("common.logout")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        ) : (
          <button
            disabled={busy}
            onClick={() => void onLogin()}
            className="group relative flex w-full items-center gap-3 overflow-hidden rounded-xl border border-cyan-400/30 bg-gradient-to-r from-cyan-500/15 via-cyan-500/5 to-violet-500/10 p-3 text-left transition-all hover:border-cyan-300/60 hover:shadow-[0_0_25px_-8px_hsl(187_100%_55%/0.7)]"
          >
            <div className="grid h-9 w-9 place-items-center rounded-full border border-cyan-400/40 bg-slate-950/60">
              <LogIn className="h-4 w-4 text-cyan-300" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="text-sm font-semibold text-cyan-100">{t("sidebar.loginTitle")}</div>
              <div className="font-mono text-[10px] uppercase tracking-[0.2em] text-slate-500">
                {t("sidebar.loginSubtitle")}
              </div>
            </div>
            <ChevronRight className="h-3.5 w-3.5 text-cyan-300/70 transition-transform group-hover:translate-x-0.5" />
          </button>
        )}
        <div className="mt-1 text-center font-mono text-[10px] uppercase tracking-[0.3em] text-slate-700">
          {t("app.versionLine")}
        </div>
      </div>
    </aside>
  );
}

function planLabel(plan: string) {
  const normalized = plan.trim().toLowerCase();
  if (!normalized || normalized === "free") return "Starter";
  return normalized.slice(0, 1).toUpperCase() + normalized.slice(1);
}

function Logo() {
  return (
    <div className="relative grid h-10 w-10 place-items-center rounded-xl border border-cyan-400/30 bg-gradient-to-br from-cyan-400/25 via-cyan-500/10 to-violet-500/15 shadow-[0_0_25px_-4px_hsl(187_100%_55%/0.7)]">
      <svg viewBox="0 0 24 24" className="h-5 w-5 text-cyan-200" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 2 L4 14 h6 l-2 8 L20 10 h-6 z" fill="currentColor" fillOpacity="0.15" />
      </svg>
      <span className="absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full bg-emerald-400 ring-2 ring-slate-950" />
    </div>
  );
}
