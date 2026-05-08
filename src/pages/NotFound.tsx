import { useLocation } from "react-router-dom";
import { useEffect } from "react";
import { useI18n } from "@/i18n";
import { captureTelemetry } from "@/services/telemetry";

const NotFound = () => {
  const location = useLocation();
  const { t } = useI18n();

  useEffect(() => {
    captureTelemetry({ name: "route_not_found", category: "navigation", properties: { path: location.pathname } });
  }, [location.pathname]);

  return (
    <div className="flex min-h-screen items-center justify-center bg-muted">
      <div className="text-center">
        <h1 className="mb-4 text-4xl font-bold">{t("notFound.title")}</h1>
        <p className="mb-4 text-xl text-muted-foreground">{t("notFound.message")}</p>
        <a href="#/" className="text-primary underline hover:text-primary/90">
          {t("notFound.backHome")}
        </a>
      </div>
    </div>
  );
};

export default NotFound;
