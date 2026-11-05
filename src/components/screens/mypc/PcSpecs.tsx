import { useI18n } from "@/i18n";
import { formatDuration, formatGigabytes, formatMegabytes, shortCpuName, shortGpuName } from "@/lib/format";
import { useLiveTelemetry } from "@/features/telemetry/useLiveTelemetry";

/**
 * Specs do PC no canto do header do Meu PC (board Tela-MeuPC). O agente não
 * informa o nome do PC nem a versão do Windows; só entra o que ele mede.
 */
export function PcSpecs() {
  const { t } = useI18n();
  const telemetry = useLiveTelemetry();
  if (!telemetry) return null;
  const gpu = shortGpuName(telemetry.gpu_name, "");
  const parts = [
    shortCpuName(telemetry.cpu_name, ""),
    gpu ? (telemetry.vram_gb ? `${gpu} ${formatGigabytes(telemetry.vram_gb)}` : gpu) : "",
    telemetry.ram_total_mb ? formatMegabytes(telemetry.ram_total_mb) : "",
    telemetry.system_uptime_seconds ? t("mypc.specs.uptime", { time: formatDuration(telemetry.system_uptime_seconds) }) : "",
  ].filter(Boolean);
  if (parts.length === 0) return null;
  return <span className="num hidden min-w-0 max-w-[360px] truncate text-xs text-fg-2 lg:inline">{parts.join(" · ")}</span>;
}
