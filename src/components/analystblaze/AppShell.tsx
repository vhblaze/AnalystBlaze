import { Suspense, lazy, useCallback, useMemo, useState } from "react";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { useAuth } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useI18n } from "@/i18n";

export type ViewKey = "dashboard" | "telemetry" | "insights" | "controls" | "settings";

const Dashboard = lazy(() => import("./views/Dashboard").then((module) => ({ default: module.Dashboard })));
const Telemetry = lazy(() => import("./views/Telemetry").then((module) => ({ default: module.Telemetry })));
const Insights = lazy(() => import("./views/Insights").then((module) => ({ default: module.Insights })));
const LocalControls = lazy(() => import("./views/LocalControls").then((module) => ({ default: module.LocalControls })));
const Settings = lazy(() => import("./views/Settings").then((module) => ({ default: module.Settings })));

export function AppShell() {
  const [view, setView] = useState<ViewKey>("dashboard");
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const auth = useAuth();
  const telemetry = useAgentTelemetry();
  const { t } = useI18n();
  const track = useTelemetry("navigation");

  const titles = useMemo<Record<ViewKey, string>>(
    () => ({
      dashboard: t("nav.dashboard"),
      telemetry: t("nav.telemetry"),
      insights: t("nav.insights"),
      controls: t("nav.controls"),
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

  const requestConfirmation = useCallback((request: Omit<ConfirmRequest, "resolve">) => {
    return new Promise<boolean>((resolve) => {
      setConfirmRequest({ ...request, resolve });
    });
  }, []);

  const closeConfirmation = useCallback((approved: boolean) => {
    setConfirmRequest((current) => {
      current?.resolve(approved);
      return null;
    });
  }, []);

  const runConfirmed = useCallback(
    async (request: Omit<ConfirmRequest, "resolve">, action: () => Promise<unknown>) => {
      const approved = await requestConfirmation(request);
      if (!approved) return false;
      await action();
      return true;
    },
    [requestConfirmation],
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
                <Dashboard
                  user={auth.user}
                  status={auth.status}
                  telemetry={telemetry}
                  onStartAgent={auth.start}
                  onActivateGameMode={async () => {
                    await runConfirmed(
                      {
                        title: "Ativar Modo Gamer",
                        description: "O agente pode alterar plano de energia, aplicar foco e colocar arquivos temporarios antigos em quarentena reversivel.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      auth.activateGameMode,
                    );
                  }}
                  busy={auth.busy}
                />
              </Suspense>
            ) : view === "telemetry" ? (
              <Suspense fallback={<ViewFallback />}>
                <Telemetry
                  latestSample={auth.sample ?? telemetry}
                  agentMode={auth.status?.mode ?? telemetry?.telemetry_mode}
                  isReady={Boolean(auth.status?.authenticated && auth.status.registered)}
                  busy={auth.busy}
                  onCollectSample={auth.collectSample}
                  onSetTelemetryMode={auth.setTelemetryMode}
                />
              </Suspense>
            ) : view === "insights" ? (
              <Suspense fallback={<ViewFallback />}>
                <Insights />
              </Suspense>
            ) : view === "controls" ? (
              <Suspense fallback={<ViewFallback />}>
                <LocalControls
                  busy={auth.busy}
                  onRestoreOptimizations={() =>
                    runConfirmed(
                      {
                        title: "Restaurar snapshots",
                        description: "O agente vai tentar desfazer alteracoes pendentes de energia, limpeza, apps de inicializacao e servicos.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      auth.restoreOptimizations,
                    )
                  }
                  onDisableStartup={(name, location) =>
                    runConfirmed(
                      {
                        title: `Desativar ${name}`,
                        description: `Remove o app da inicializacao do Windows em ${location ?? "registro detectado"} e cria snapshot para restaurar depois.`,
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.disableStartup(name, location),
                    )
                  }
                  onRestoreStartup={(name) =>
                    runConfirmed(
                      {
                        title: name ? `Restaurar ${name}` : "Restaurar apps de inicializacao",
                        description: "Restaura o valor original salvo no snapshot local do Registro.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restoreStartup(name),
                    )
                  }
                  onStopService={(name) =>
                    runConfirmed(
                      {
                        title: `Parar servico ${name}`,
                        description: "O servico sera parado apenas se passar pela denylist local. Um snapshot guarda se ele estava rodando antes.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.stopService(name),
                    )
                  }
                  onRestoreService={(name) =>
                    runConfirmed(
                      {
                        title: name ? `Restaurar servico ${name}` : "Restaurar servicos",
                        description: "Tenta religar apenas servicos que estavam rodando antes da acao.",
                        risk: "seguro",
                        snapshot: false,
                      },
                      () => auth.restoreService(name),
                    )
                  }
                  onSetPowerPlan={(plan) =>
                    runConfirmed(
                      {
                        title: plan === "high_performance" ? "Ativar alto desempenho" : plan === "power_saver" ? "Ativar economia de energia" : "Ativar plano equilibrado",
                        description: "Altera o plano de energia do Windows e cria snapshot local para restaurar o plano anterior depois.",
                        risk: "sensivel",
                        snapshot: true,
                      },
                      () => auth.setPowerPlan(plan),
                    )
                  }
                />
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
                  onOpenAccountSettings={auth.openAccountSettings}
                  onStartAgent={auth.start}
                  onCollectSample={auth.collectSample}
                />
              </Suspense>
            )}
          </div>
        </div>
      </main>
      {confirmRequest && (
        <ConfirmationDialog
          request={confirmRequest}
          onCancel={() => closeConfirmation(false)}
          onConfirm={() => closeConfirmation(true)}
        />
      )}
    </div>
  );
}

type ConfirmRequest = {
  title: string;
  description: string;
  risk: string;
  snapshot: boolean;
  resolve: (approved: boolean) => void;
};

function ConfirmationDialog({
  request,
  onCancel,
  onConfirm,
}: {
  request: ConfirmRequest;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/75 px-4 backdrop-blur-sm">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={request.title}
        className="w-full max-w-lg rounded-2xl border border-cyan-400/20 bg-slate-950 p-6 shadow-[0_25px_80px_-30px_hsl(187_100%_55%/0.7)]"
      >
        <div className="font-mono text-[10px] uppercase tracking-[0.25em] text-amber-300">
          confirmacao local
        </div>
        <h2 className="mt-2 text-xl font-semibold text-slate-50">{request.title}</h2>
        <p className="mt-2 text-sm leading-relaxed text-slate-400">{request.description}</p>
        <div className="mt-4 grid gap-2 sm:grid-cols-2">
          <div className="rounded-xl border border-cyan-500/10 bg-slate-900/70 p-3">
            <span className="block font-mono text-[10px] uppercase tracking-widest text-slate-500">risco</span>
            <strong className="mt-1 block text-sm text-slate-100">{request.risk}</strong>
          </div>
          <div className="rounded-xl border border-cyan-500/10 bg-slate-900/70 p-3">
            <span className="block font-mono text-[10px] uppercase tracking-widest text-slate-500">snapshot</span>
            <strong className="mt-1 block text-sm text-slate-100">{request.snapshot ? "obrigatorio" : "nao altera snapshot"}</strong>
          </div>
        </div>
        <div className="mt-6 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-xl border border-slate-600/60 px-4 py-2 text-sm font-medium text-slate-300 transition hover:border-slate-400/70"
          >
            Cancelar
          </button>
          <button
            onClick={onConfirm}
            className="rounded-xl border border-cyan-400/50 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-100 transition hover:bg-cyan-400/15"
          >
            Confirmar
          </button>
        </div>
      </section>
    </div>
  );
}

function ViewFallback() {
  return <div className="glass-panel h-44 animate-pulse" />;
}
