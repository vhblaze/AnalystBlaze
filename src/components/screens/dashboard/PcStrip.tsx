import { ArrowRight } from "lucide-react";
import { Card } from "@/components/ui/card";
import { EmptyState } from "@/components/ui/empty-state";
import { Kpi } from "@/components/ui/kpi";
import { useI18n } from "@/i18n";
import {
  formatBytes,
  formatGhz,
  formatMegabytes,
  formatPercent,
  formatTemperature,
  shortGpuName,
} from "@/lib/format";
import type { AgentTelemetrySnapshot, DiskVolumeInfo } from "@/services/tauri/agent";
import { THRESHOLDS } from "@/features/telemetry/weighingNow";

/**
 * Faixa do PC: CPU, GPU, Memória e Disco, com o número que pede atenção em
 * âmbar e "Meu PC →". O "livre" do disco vem da mesma leitura da aba Disco
 * (doc 14 Q4).
 */
export function PcStrip({
  telemetry,
  volume,
  onOpenMyPc,
}: {
  telemetry: AgentTelemetrySnapshot | null;
  volume: DiskVolumeInfo | null;
  onOpenMyPc: () => void;
}) {
  const { t } = useI18n();

  if (!telemetry) {
    return (
      <Card padding="md">
        <EmptyState variant="inline" title={t("dashboard.d3.stripEmptyTitle")} reason={t("dashboard.d3.stripEmptyReason")} />
      </Card>
    );
  }

  const gpuName = shortGpuName(telemetry.gpu_name, "");
  const memoryPercent = telemetry.ram_usage_percent ?? null;
  const diskFree = volume
    ? formatBytes(volume.availableBytes)
    : telemetry.disk_total_gb && telemetry.disk_used_gb != null
      ? formatBytes((telemetry.disk_total_gb - telemetry.disk_used_gb) * 1024 ** 3)
      : null;

  return (
    <Card padding="md" className="flex items-center gap-6">
      <div className="grid flex-1 grid-cols-4 divide-x divide-line">
        <Kpi
          className="pr-4"
          label={t("dashboard.d3.cpu")}
          value={formatPercent(telemetry.cpu_usage)}
          hint={formatGhz(telemetry.cpu_frequency_mhz, "")}
          tone={telemetry.cpu_usage >= THRESHOLDS.cpuPercent ? "warn" : "default"}
        />
        <Kpi
          className="px-4"
          label={gpuName ? `${t("dashboard.d3.gpu")} · ${gpuName}` : t("dashboard.d3.gpu")}
          value={
            telemetry.gpu_usage_available ? (
              formatPercent(telemetry.gpu_usage)
            ) : (
              <span className="text-sm font-normal text-fg-3">{t("dashboard.d3.noReadingOnThisPc")}</span>
            )
          }
          hint={formatTemperature(telemetry.gpu_temperature, telemetry.gpu_temperature_available, "")}
        />
        <Kpi
          className="px-4"
          label={t("dashboard.d3.memory")}
          value={formatMegabytes(telemetry.ram_usage_mb)}
          hint={telemetry.ram_total_mb ? `/ ${formatMegabytes(telemetry.ram_total_mb)}` : null}
          tone={memoryPercent !== null && memoryPercent >= THRESHOLDS.memoryPercent ? "warn" : "default"}
        />
        <Kpi
          className="pl-4"
          label={t("dashboard.d3.disk")}
          value={diskFree ?? <span className="text-sm font-normal text-fg-3">{t("dashboard.d3.noReadingOnThisPc")}</span>}
          hint={diskFree ? t("dashboard.d3.free") : null}
        />
      </div>
      <button
        type="button"
        onClick={onOpenMyPc}
        className="flex shrink-0 items-center gap-1 text-[13px] font-medium text-accent-2 underline underline-offset-4 hover:text-accent"
      >
        {t("nav.mypc")}
        <ArrowRight className="h-3.5 w-3.5" aria-hidden />
      </button>
    </Card>
  );
}
