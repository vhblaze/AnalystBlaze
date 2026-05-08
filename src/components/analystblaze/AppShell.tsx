import { Suspense, lazy, useCallback, useMemo, useState } from "react";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { useAuth } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";

export type ViewKey = "dashboard" | "telemetry" | "insights" | "settings";

const Dashboard = lazy(() => import("./views/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("./views/Telemetry").then((module) => ({ default: module.Telemetry })));
const Insights = lazy(() => import("./views/Insights").then((module) => ({ default: module.Insights })));
const Settings = lazy(() => import("./views/Settings").then((module) => ({ default: module.Settings })));

export function AppShell() {
  const [view, setView] = useState<ViewKey>("dashboard");
  const auth = useAuth();
  const { t } = useI18n();
  const track = useTelemetry("navigation");

  const titles = useMemo<Record<ViewKey, string>>(
    () => ({
      dashboard: t("nav.dashboard"),
      telemetry: t("nav.telemetry"),
      insights: t("nav.insights"),
      settings: t("nav.settings"),
    }),
    [t],
  );

  const handleViewChange = useCallback(
    (nextView: ViewKey) => {
      setView(nextView);
      track("navigation_change", { view: nextView });
    },
    [track],
  );

  return (
    <div className="relative flex h-screen w-full overflow-hidden text-slate-100">
      <div className="pointer-events-none absolute inset-0 grid-bg opacity-60" />
      <div className="pointer-events-none absolute inset-0 scanline opacity-40" />
      <div className="pointer-events-none absolute inset-0 noise opacity-[0.035] mix-blend-overlay" />

      <Sidebar
        view={view}
        onChange={handleViewChange}
        user={auth.user}
        status={auth.status}
        busy={auth.busy}
        onLogin={auth.login}
        onLogout={auth.logout}
      />

      <main className="relative z-10 flex flex-1 flex-col overflow-hidden">
        <TopBar title={titles[view]} user={auth.user} status={auth.status} />
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto max-w-6xl px-5 py-6 sm:px-8 lg:px-10 lg:py-8">
            {!auth.ready ? (
              <div className="text-sm text-slate-500">{t("app.loading")}</div>
            ) : view === "dashboard" ? (
              <Suspense fallback={<ViewFallback />}>
                <Dashboard user={auth.user} status={auth.status} onStartAgent={auth.start} busy={auth.busy} />
              </Suspense>
            ) : view === "telemetry" ? (
              <Suspense fallback={<ViewFallback />}>
                <Telemetry latestSample={auth.sample} onCollectSample={auth.collectSample} />
              </Suspense>
            ) : view === "insights" ? (
              <Suspense fallback={<ViewFallback />}>
                <Insights />
              </Suspense>
            ) : (
              <Suspense fallback={<ViewFallback />}>
                <Settings
                  user={auth.user}
                  status={auth.status}
                  message={auth.message}
                  busy={auth.busy}
                  onLogin={auth.login}
                  onLogout={auth.logout}
                  onStartAgent={auth.start}
                  onSetTelemetryMode={auth.setTelemetryMode}
                  onCollectSample={auth.collectSample}
                />
              </Suspense>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

function ViewFallback() {
  return <div className="glass-panel h-44 animate-pulse" />;
}
