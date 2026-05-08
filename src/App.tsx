import { useEffect } from "react";
import { HashRouter, Route, Routes } from "react-router-dom";
import { Toaster as Sonner } from "@/components/ui/sonner";
import { Toaster } from "@/components/ui/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { I18nProvider } from "@/i18n";
import { captureTelemetry, captureUiError, startTelemetryService } from "@/services/telemetry";
import Index from "./pages/Index.tsx";
import NotFound from "./pages/NotFound.tsx";

function AppContent() {
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
