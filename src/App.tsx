import { useEffect } from "react";
import { HashRouter, Route, Routes } from "react-router-dom";
import { Toaster as Sonner } from "@/components/ui/sonner";
import { Toaster } from "@/components/ui/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { I18nProvider } from "@/i18n";
import { captureTelemetry, captureUiError, startTelemetryService } from "@/services/telemetry";
import { isTauriRuntime } from "@/services/tauri/agent";
import Index from "./pages/Index.tsx";
import NotFound from "./pages/NotFound.tsx";

/** Sets [data-window-unfocused] on <html> whenever the OS reports this
 * window isn't the focused one - see index.css, which uses it to pause
 * every CSS animation (animate-pulse/animate-spin dots, the float/shimmer
 * keyframes) while unfocused. Found live: a real user's WebView2 renderer +
 * gpu-process kept costing ~80% of a CPU core while AnalystBlaze sat open
 * behind Valorant during a match - unfocused, not minimized, with nothing
 * anywhere pausing the dashboard's animations just because the window
 * wasn't the one on top. document.visibilitychange doesn't fire for this
 * case (the webview is still "visible", just not focused), so this needs
 * Tauri's own window focus event instead.
 */
function useWindowFocusAttribute() {
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const setUnfocused = (unfocused: boolean) => {
      document.documentElement.toggleAttribute("data-window-unfocused", unfocused);
    };

    import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        if (cancelled) return;
        const win = getCurrentWindow();
        setUnfocused(!(await win.isFocused()));
        unlisten = await win.onFocusChanged(({ payload: focused }) => setUnfocused(!focused));
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}

function AppContent() {
  useWindowFocusAttribute();

  useEffect(() => {
    const stopTelemetry = startTelemetryService();
    captureTelemetry({ name: "app_open", category: "lifecycle" });

    const handleError = (event: ErrorEvent) => captureUiError(event.error ?? event.message, "window_error");
    const handleUnhandledRejection = (event: PromiseRejectionEvent) =>
      captureUiError(event.reason, "unhandled_rejection");

    window.addEventListener("error", handleError);
    window.addEventListener("unhandledrejection", handleUnhandledRejection);

    return () => {
      window.removeEventListener("error", handleError);
      window.removeEventListener("unhandledrejection", handleUnhandledRejection);
      stopTelemetry();
    };
  }, []);

  return (
    <TooltipProvider>
      <Toaster />
      <Sonner />
      <HashRouter>
        <Routes>
          <Route path="/" element={<Index />} />
          <Route path="*" element={<NotFound />} />
        </Routes>
      </HashRouter>
    </TooltipProvider>
  );
}

const App = () => (
  <I18nProvider>
    <AppContent />
  </I18nProvider>
);

export default App;
