import { useCallback, useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { LoggedOutNotice } from "./LoggedOutNotice";
import { UpdateNotice } from "./UpdateNotice";
import { useAgentTelemetry } from "@/hooks/useAgentTelemetry";
import { startTelemetryHistory } from "@/features/telemetry/telemetryHistory";
import { useAuth } from "@/hooks/useAuth";
import { useTelemetry } from "@/hooks/useTelemetry";
import { useUpdater } from "@/hooks/useUpdater";
import { useI18n } from "@/i18n";
import { splitBackendErrorCode } from "@/lib/backend-errors";
import { DEFAULT_ROUTE, route, routeId, routeTitleKeys, sameRoute, type Route } from "@/lib/routes";
import { Sidebar } from "@/components/shell/Sidebar";
import { Header } from "@/components/shell/Header";
import { CommandPalette, useCommandItems } from "@/components/shell/CommandPalette";
import { Notifications, useNotificationItems } from "@/components/shell/Notifications";
import { ScreenOutlet } from "@/components/shell/ScreenOutlet";
import { useAgentPrompts } from "@/components/shell/useAgentPrompts";
import { useConfirmation } from "@/components/shell/useConfirmation";
import { ConfirmationDialog } from "@/components/shell/dialogs/ConfirmationDialog";
import { DeviceLimitDialog } from "@/components/shell/dialogs/DeviceLimitDialog";
import { EmailVerificationNotice } from "@/components/shell/dialogs/EmailVerificationNotice";
import { RemoteConfirmationNotice } from "@/components/shell/dialogs/RemoteConfirmationNotice";
import { RestoreToDateButton } from "@/components/screens/history/History";
import { PcSpecs } from "@/components/screens/mypc/PcSpecs";

// Mantido para quem ainda importa daqui.
export { splitBackendErrorCode };

/**
 * Casca do app (doc 15 §5): sidebar + header + a tela da rota atual, e os
 * avisos que podem aparecer por cima de qualquer tela. A lógica de cada parte
 * mora em components/shell.
 */
export function AppShell() {
  const [current, setCurrent] = useState<Route>(DEFAULT_ROUTE);
  // Histórico de 1 hora da aba Desempenho: começa a gravar quando o app abre.
  useEffect(() => {
    startTelemetryHistory();
  }, []);
  // Acessibilidade (Configurações): vale desde a abertura do app, não só
  // depois que a tela de Configurações é aberta.
  useEffect(() => {
    try {
      const root = document.documentElement;
      root.classList.toggle("ab-high-contrast", localStorage.getItem("analystblaze.accessibility.highContrast") === "1");
      root.classList.toggle("ab-reduce-motion", localStorage.getItem("analystblaze.accessibility.reduceMotion") === "1");
    } catch {
      // Sem localStorage: fica o padrão.
    }
  }, []);
  const auth = useAuth();
  const telemetry = useAgentTelemetry();
  const updater = useUpdater();
  const { t } = useI18n();
  const track = useTelemetry("navigation");
  const { confirmRequest, requestConfirmation, closeConfirmation, runConfirmed } = useConfirmation();
  const prompts = useAgentPrompts({ auth, updater, requestConfirmation });

  const navigate = useCallback(
    (next: Route) => {
      setCurrent((previous) => (sameRoute(previous, next) ? previous : next));
      track("navigation_change", { view: routeId(next) });
    },
    [track],
  );

  const commandItems = useCommandItems({ auth, navigate, runConfirmed });
  const notificationItems = useNotificationItems({
    remoteConfirmationQueue: prompts.remoteConfirmationQueue,
    user: auth.user,
    updateStatus: updater.status,
    announcements: prompts.announcements,
    dismissedAnnouncementIds: prompts.dismissedAnnouncementIds,
  });

  const onNotificationClick = (id: string) => {
    if (id === "update-available") return prompts.handleUpdateNow();
    if (id.startsWith("announcement:")) return prompts.dismissAnnouncement(id.slice("announcement:".length));
    navigate(route("dashboard"));
  };

  const loggedOutError = (() => {
    // Bloco 5.4: the server ended the session and said why - show that.
    if (auth.message.key.startsWith("agent.messages.sessionEnded.")) {
      return { code: null, message: t(auth.message.key) };
    }
    if (auth.message.key !== "agent.messages.error") return null;
    return splitBackendErrorCode(t(auth.message.key, auth.message.params));
  })();
  const remoteQueue = prompts.remoteConfirmationQueue;
  const deviceLimitChoices = prompts.deviceLimitChoices;

  return (
    <div className="relative flex h-screen w-full overflow-hidden bg-bg text-fg">
      <Sidebar
        current={current}
        onNavigate={navigate}
        user={auth.user}
        status={auth.status}
        busy={auth.busy}
        onLogin={auth.login}
        onLogout={auth.logout}
      />

      <main className="relative z-10 flex flex-1 flex-col overflow-hidden">
        <Header
          title={routeTitleKeys(current).map((key) => t(key))}
          right={
            <>
              {current.screen === "mypc" && <PcSpecs />}
              {current.screen === "history" && <RestoreToDateButton />}
              {current.screen === "controls" && current.section && (
                <Button size="sm" variant="link" onClick={() => navigate(route("controls"))}>
                  <ArrowLeft />
                  {t("nav.controls")}
                </Button>
              )}
              <CommandPalette items={commandItems} />
              <Notifications items={notificationItems} onItemClick={onNotificationClick} />
            </>
          }
        />
        <div className="flex-1 overflow-y-auto">
          <div className="mx-auto max-w-6xl px-10 py-8">
            {remoteQueue.length > 0 && (
              <RemoteConfirmationNotice request={remoteQueue[remoteQueue.length - 1]} count={remoteQueue.length} />
            )}
            <ScreenOutlet
              current={current}
              navigate={navigate}
              auth={auth}
              telemetry={telemetry}
              runConfirmed={runConfirmed}
              requestConfirmation={requestConfirmation}
              shadowConsentNeeded={prompts.shadowConsentNeeded}
              resolveShadowConsent={prompts.resolveShadowConsent}
            />
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
      {auth.user && !auth.user.emailVerified && !prompts.emailNoticeDismissed && (
        <EmailVerificationNotice
          daysRemaining={auth.user.emailVerificationDaysRemaining}
          onVerify={() => {
            prompts.setEmailNoticeDismissed(true);
            void auth.openAccountSettings();
          }}
          onLater={() => prompts.setEmailNoticeDismissed(true)}
        />
      )}
      {deviceLimitChoices.length > 0 && (
        <DeviceLimitDialog
          devices={deviceLimitChoices}
          busy={prompts.replacingDevice}
          onReplace={(hwId) => void prompts.handleReplaceDevice(hwId)}
          onCancel={() => void prompts.handleCancelDeviceLimit()}
        />
      )}
      <LoggedOutNotice
        visible={auth.ready && !auth.status?.authenticated && deviceLimitChoices.length === 0}
        busy={auth.busy}
        onLogin={() => void auth.login()}
        errorMessage={loggedOutError?.message ?? null}
        showManageDevices={loggedOutError?.code === "DEVICE_LIMIT_REACHED"}
        onManageDevices={() => void auth.openAccountSettings()}
      />
      {/* Suppressed while logged out - stacking it with LoggedOutNotice would
          overlap two full-screen dialogs, and there's nothing to update to
          right now anyway without a session. */}
      {(!auth.ready || auth.status?.authenticated) && (
        <UpdateNotice
          status={updater.status}
          busy={auth.busy}
          onUpdateNow={prompts.handleUpdateNow}
          onLater={prompts.handleUpdateLater}
        />
      )}
    </div>
  );
}
