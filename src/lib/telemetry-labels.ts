/** Human label for a temperature sensor source code, shared by Dashboard
 * and Telemetry so the same reading isn't described differently depending
 * on which screen you're looking at (they used to keep separate copies
 * that had drifted - e.g. "sysinfo" on one screen, "sensor do sistema" on
 * the other, for the exact same `sysinfo_cpu_sensor` value). */
export function temperatureSourceLabel(source?: string | null): string {
  if (source === "nvml") return "NVML";
  if (source === "sysinfo_cpu_sensor") return "sensor do sistema";
  if (source === "sysinfo_gpu_sensor") return "sensor GPU do sistema";
  if (source === "sysinfo_component_max") return "componente mais quente";
  if (source === "libre_hardware_monitor") return "LibreHardwareMonitor";
  if (source === "open_hardware_monitor") return "OpenHardwareMonitor";
  if (source === "acpi_thermal_zone") return "ACPI thermal zone";
  if (source === "hardware_monitor") return "monitor de hardware";
  if (source === "external_wmi") return "WMI";
  return "fonte local";
}
