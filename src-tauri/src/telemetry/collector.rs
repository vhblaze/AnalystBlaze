use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sysinfo::{Components, Disks, ProcessesToUpdate, System};

use super::advanced::{collect_advanced_telemetry, AdvancedTelemetry};
use super::network::{best_latency_ms, collect_network_sample, NetworkDiagnostics};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_gb: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_used_gb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub hw_hash: String,
    pub machine_type: String,
    pub processor: String,
    pub cpu_cores: i32,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub ram_gb: f64,
    pub os_version: String,
    pub system_info: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySample {
    pub event_timestamp: i64,
    pub cpu_usage: f64,
    pub cpu_temperature: f64,
    pub cpu_temperature_available: bool,
    pub cpu_temperature_source: Option<String>,
    pub cpu_temperature_methods: Vec<CpuTemperatureMethod>,
    pub gpu_usage: f64,
    pub gpu_usage_available: bool,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub vram_used_gb: Option<f64>,
    pub vram_usage_percent: Option<f64>,
    pub ram_usage_mb: f64,
    pub ram_total_mb: f64,
    pub ram_usage_percent: f64,
    pub gpu_temperature: f64,
    pub gpu_temperature_available: bool,
    pub latency_ms: f64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub disk_usage_percent: f64,
    pub active_processes: usize,
    pub system_uptime_seconds: u64,
    pub active_window: Option<String>,
    pub idle_seconds: u64,
    pub advanced: AdvancedTelemetry,
    pub network: NetworkDiagnostics,
    pub context_state: Value,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuTemperatureMethod {
    pub source: String,
    pub label: Option<String>,
    pub value_c: Option<f64>,
    pub available: bool,
}

#[derive(Debug, Clone, Default)]
struct CpuTemperatureReading {
    value_c: Option<f64>,
    source: Option<String>,
    methods: Vec<CpuTemperatureMethod>,
}

pub struct TelemetryCollector {
    system: System,
    components: Components,
    disks: Disks,
    gpu_devices: Vec<GpuInfo>,
    gpu_usage_reader: Option<GpuUsageReader>,
    nvidia_sensor_reader: Option<NvidiaSensorReader>,
    collection_count: u64,
    advanced_cache: AdvancedTelemetry,
    advanced_refreshed_at: i64,
    network_cache: NetworkDiagnostics,
    network_refreshed_at: i64,
    cpu_temperature_cache: CpuTemperatureReading,
    cpu_temperature_refreshed_at: i64,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            components: Components::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            gpu_devices: detect_gpu_devices(),
            gpu_usage_reader: GpuUsageReader::new(),
            nvidia_sensor_reader: NvidiaSensorReader::new(),
            collection_count: 0,
            advanced_cache: AdvancedTelemetry::default(),
            advanced_refreshed_at: 0,
            network_cache: NetworkDiagnostics::default(),
            network_refreshed_at: 0,
            cpu_temperature_cache: CpuTemperatureReading::default(),
            cpu_temperature_refreshed_at: 0,
        }
    }

    pub fn collect(&mut self) -> TelemetrySample {
        self.collection_count = self.collection_count.saturating_add(1);
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.components.refresh(false);
        if self.collection_count == 1 || self.collection_count % 5 == 0 {
            self.system.refresh_processes(ProcessesToUpdate::All, true);
            self.disks.refresh(false);
        }

        let cpu_usage = self.system.global_cpu_usage() as f64;
        let ram_usage_mb = bytes_to_mb(self.system.used_memory());
        let ram_total_mb = bytes_to_mb(self.system.total_memory());
        let ram_usage_percent = if ram_total_mb > 0.0 {
            clamp_percent((ram_usage_mb / ram_total_mb) * 100.0)
        } else {
            0.0
        };
        let cpu_temperature_reading = self.detect_cpu_temperature();
        let cpu_temperature_available = cpu_temperature_reading.value_c.is_some();
        let cpu_temperature = cpu_temperature_reading.value_c.unwrap_or_default();
        let cpu_temperature_source = cpu_temperature_reading.source.clone();
        let cpu_temperature_methods = cpu_temperature_reading.methods.clone();
        let nvidia_sensors = self
            .nvidia_sensor_reader
            .as_ref()
            .and_then(NvidiaSensorReader::sample);
        let gpu_temperature = nvidia_sensors
            .as_ref()
            .and_then(|sample| sample.temperature_c)
            .or_else(|| self.detect_gpu_temperature());
        let gpu_temperature_available = gpu_temperature.is_some();
        let gpu_temperature = gpu_temperature.unwrap_or_default() as f64;
        let gpu_usage = nvidia_sensors
            .as_ref()
            .and_then(|sample| sample.utilization_percent)
            .or_else(|| {
                self.gpu_usage_reader
                    .as_mut()
                    .and_then(GpuUsageReader::sample)
            });
        let primary_gpu = self.primary_gpu();
        let gpu_name = primary_gpu.name.clone();
        let vram_gb = primary_gpu.vram_gb;
        let vram_used_gb = nvidia_sensors
            .as_ref()
            .and_then(|sample| sample.vram_used_gb)
            .or(primary_gpu.vram_used_gb);
        let vram_usage_percent = vram_used_gb.and_then(|used| {
            if vram_gb > 0.0 {
                Some(clamp_percent((used / vram_gb) * 100.0))
            } else {
                None
            }
        });
        let (disk_used_gb, disk_total_gb, disk_usage_percent) = self.disk_usage();
        let active_processes = self.system.processes().len();
        let system_uptime_seconds = System::uptime();
        let active_window = active_window_title();
        let idle_seconds = idle_seconds();
        let process_names = running_process_names(&self.system);
        let advanced = self.advanced_telemetry();
        let network = self.network_diagnostics();
        let latency_ms = best_latency_ms(&network);
        let local_context = detect_local_context(
            active_window.as_deref(),
            &process_names,
            gpu_usage.unwrap_or_default(),
            cpu_usage,
            idle_seconds,
        );
        let detected_activity = local_context
            .get("activity")
            .cloned()
            .unwrap_or_else(|| json!("unknown"));

        TelemetrySample {
            event_timestamp: chrono::Utc::now().timestamp(),
            cpu_usage: clamp_percent(cpu_usage),
            cpu_temperature,
            cpu_temperature_available,
            cpu_temperature_source: cpu_temperature_source.clone(),
            cpu_temperature_methods: cpu_temperature_methods.clone(),
            gpu_usage: gpu_usage.unwrap_or_default(),
            gpu_usage_available: gpu_usage.is_some(),
            gpu_name: gpu_name.clone(),
            vram_gb,
            vram_used_gb,
            vram_usage_percent,
            ram_usage_mb,
            ram_total_mb,
            ram_usage_percent,
            gpu_temperature,
            gpu_temperature_available,
            latency_ms,
            disk_used_gb,
            disk_total_gb,
            disk_usage_percent,
            active_processes,
            system_uptime_seconds,
            active_window: active_window.clone(),
            idle_seconds,
            advanced: advanced.clone(),
            network: network.clone(),
            context_state: json!({
                "mode": "observed",
                "activity": detected_activity,
                "local_context": local_context,
                "host_name": System::host_name().unwrap_or_else(|| "unknown".to_string()),
                "gpu_name": gpu_name.clone(),
                "vram_gb": vram_gb,
                "cpu_temperature": cpu_temperature,
                "cpu_temperature_available": cpu_temperature_available,
                "cpu_temperature_source": cpu_temperature_source.clone(),
                "cpu_temperature_methods": cpu_temperature_methods.clone(),
                "gpu_temperature_available": gpu_temperature_available,
                "ram_usage_percent": ram_usage_percent,
                "disk_usage_percent": disk_usage_percent,
                "advanced": advanced.clone(),
                "network": {
                    "connected": network.connected,
                    "adapter_type": network.adapter_type.clone(),
                    "wifi_signal_percent": network.wifi_signal_percent,
                    "latency_ms": latency_ms,
                    "jitter_ms": network.jitter_ms,
                    "packet_loss_percent": network.packet_loss_percent,
                    "recommendations": network.recommendations.clone(),
                },
            }),
            details: json!({
                "gpu_name": gpu_name,
                "vram_gb": vram_gb,
                "vram_used_gb": vram_used_gb,
                "vram_usage_percent": vram_usage_percent,
                "cpu_temperature": cpu_temperature,
                "cpu_temperature_available": cpu_temperature_available,
                "cpu_temperature_source": cpu_temperature_source,
                "cpu_temperature_methods": cpu_temperature_methods,
                "ram_total_mb": ram_total_mb,
                "ram_usage_percent": ram_usage_percent,
                "gpu_devices": self.gpu_devices,
                "gpu_temperature": gpu_temperature,
                "gpu_temperature_available": gpu_temperature_available,
                "gpu_usage_available": gpu_usage.is_some(),
                "disk_used_gb": disk_used_gb,
                "disk_total_gb": disk_total_gb,
                "disk_usage_percent": disk_usage_percent,
                "active_processes": active_processes,
                "system_uptime_seconds": system_uptime_seconds,
                "idle_seconds": idle_seconds,
                "active_window": active_window,
                "process_sample": process_names.into_iter().take(30).collect::<Vec<_>>(),
                "latency_ms": latency_ms,
                "network": network,
                "advanced": advanced,
                "source": "sysinfo",
            }),
        }
    }

    pub fn hardware_profile(&self) -> HardwareProfile {
        let processor = self
            .system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        let cpu_cores = self.system.cpus().len().max(1) as i32;
        let ram_gb = bytes_to_gb(self.system.total_memory());
        let os_version = format!(
            "{} {}",
            System::name().unwrap_or_else(|| "Windows".to_string()),
            System::kernel_version().unwrap_or_else(|| "unknown".to_string())
        );
        let host_name = System::host_name().unwrap_or_else(|| "unknown".to_string());
        let machine_id_hash = machine_id_hash();
        let hw_hash = stable_hw_hash(
            machine_id_hash.as_deref(),
            &host_name,
            &processor,
            ram_gb,
            cpu_cores,
            &os_version,
        );
        let primary_gpu = self.primary_gpu();
        let fingerprint_source = if machine_id_hash.is_some() {
            "windows_machine_guid"
        } else {
            "system_profile"
        };

        HardwareProfile {
            hw_hash,
            machine_type: "Windows-Desktop".to_string(),
            processor,
            cpu_cores,
            gpu_name: primary_gpu.name.clone(),
            vram_gb: primary_gpu.vram_gb,
            ram_gb,
            os_version,
            system_info: Some(json!({
                "host_name": host_name,
                "collector": "sysinfo",
                "fingerprint_version": 2,
                "fingerprint_source": fingerprint_source,
                "machine_id_hash": machine_id_hash,
                "gpu_devices": self.gpu_devices,
            })),
        }
    }

    fn primary_gpu(&self) -> GpuInfo {
        self.gpu_devices
            .iter()
            .filter(|gpu| !gpu.name.eq_ignore_ascii_case("Unknown GPU"))
            .max_by(|left, right| left.vram_gb.total_cmp(&right.vram_gb))
            .cloned()
            .unwrap_or_else(|| GpuInfo {
                name: "Unknown GPU".to_string(),
                vram_gb: 0.0,
                vram_used_gb: None,
            })
    }

    fn disk_usage(&self) -> (f64, f64, f64) {
        let total_bytes = self
            .disks
            .iter()
            .map(|disk| disk.total_space())
            .sum::<u64>();
        let available_bytes = self
            .disks
            .iter()
            .map(|disk| disk.available_space())
            .sum::<u64>();
        let used_bytes = total_bytes.saturating_sub(available_bytes);
        let used_gb = bytes_to_gb(used_bytes);
        let total_gb = bytes_to_gb(total_bytes);
        let usage_percent = if total_bytes > 0 {
            clamp_percent((used_bytes as f64 / total_bytes as f64) * 100.0)
        } else {
            0.0
        };
        (used_gb, total_gb, usage_percent)
    }

    fn detect_gpu_temperature(&self) -> Option<f32> {
        self.components.iter().find_map(|component| {
            let label = component.label().to_ascii_lowercase();
            if is_gpu_sensor_label(&label) {
                component.temperature().filter(|value| value.is_finite())
            } else {
                None
            }
        })
    }

    fn detect_cpu_temperature(&mut self) -> CpuTemperatureReading {
        let mut methods = self.sysinfo_cpu_temperature_methods();
        if let Some(method) = methods.iter().find(|method| method.available) {
            return CpuTemperatureReading {
                value_c: method.value_c,
                source: Some(method.source.clone()),
                methods,
            };
        }

        let now = chrono::Utc::now().timestamp();
        if now.saturating_sub(self.cpu_temperature_refreshed_at) >= 30 {
            let external_methods = external_cpu_temperature_methods();
            self.cpu_temperature_cache = CpuTemperatureReading::from_methods(external_methods);
            self.cpu_temperature_refreshed_at = now;
        }

        if self.cpu_temperature_cache.methods.is_empty() {
            methods.push(CpuTemperatureMethod::unavailable(
                "external_wmi",
                "sensor WMI nao exposto",
            ));
        } else {
            methods.extend(self.cpu_temperature_cache.methods.clone());
        }

        let best = CpuTemperatureReading::from_methods(methods);
        if best.value_c.is_some() {
            best
        } else {
            self.cpu_temperature_cache.clone()
        }
    }

    fn sysinfo_cpu_temperature_methods(&self) -> Vec<CpuTemperatureMethod> {
        let mut labeled = Vec::new();
        let mut fallback = Vec::new();

        for component in self.components.iter() {
            let raw_label = component.label().trim().to_string();
            let label = raw_label.to_ascii_lowercase();
            let value = component
                .temperature()
                .filter(|value| sane_temperature(*value));
            let method = CpuTemperatureMethod {
                source: if is_cpu_sensor_label(&label) || label == "computer" {
                    "sysinfo_cpu_sensor".to_string()
                } else {
                    "sysinfo_component_max".to_string()
                },
                label: Some(raw_label).filter(|value| !value.is_empty()),
                value_c: value.map(|value| value as f64),
                available: value.is_some(),
            };

            if method.source == "sysinfo_cpu_sensor" {
                labeled.push(method);
            } else if method.available {
                fallback.push(method);
            }
        }

        let mut methods = Vec::new();
        if let Some(best) = labeled.into_iter().max_by(|left, right| {
            left.value_c
                .unwrap_or_default()
                .total_cmp(&right.value_c.unwrap_or_default())
        }) {
            methods.push(best);
        } else {
            methods.push(CpuTemperatureMethod::unavailable(
                "sysinfo_cpu_sensor",
                "sensor CPU nao exposto pelo sistema",
            ));
        }

        if let Some(best) = fallback.into_iter().max_by(|left, right| {
            left.value_c
                .unwrap_or_default()
                .total_cmp(&right.value_c.unwrap_or_default())
        }) {
            methods.push(best);
        }

        methods
    }

    fn advanced_telemetry(&mut self) -> AdvancedTelemetry {
        let now = chrono::Utc::now().timestamp();
        if now.saturating_sub(self.advanced_refreshed_at) >= 300 {
            self.advanced_cache = collect_advanced_telemetry();
            self.advanced_refreshed_at = now;
        }

        self.advanced_cache.clone()
    }

    fn network_diagnostics(&mut self) -> NetworkDiagnostics {
        let now = chrono::Utc::now().timestamp();
        if now.saturating_sub(self.network_refreshed_at) >= 30 {
            self.network_cache = collect_network_sample();
            self.network_refreshed_at = now;
        }

        self.network_cache.clone()
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuTemperatureReading {
    fn from_methods(methods: Vec<CpuTemperatureMethod>) -> Self {
        let best = methods
            .iter()
            .filter(|method| method.available)
            .filter_map(|method| method.value_c.map(|value| (value, method.source.clone())))
            .max_by(|left, right| left.0.total_cmp(&right.0));

        Self {
            value_c: best.as_ref().map(|(value, _)| *value),
            source: best.map(|(_, source)| source),
            methods,
        }
    }
}

impl CpuTemperatureMethod {
    fn unavailable(source: &str, label: &str) -> Self {
        Self {
            source: source.to_string(),
            label: Some(label.to_string()),
            value_c: None,
            available: false,
        }
    }
}

fn stable_hw_hash(
    machine_id_hash: Option<&str>,
    host_name: &str,
    processor: &str,
    ram_gb: f64,
    cpu_cores: i32,
    os_version: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"analystblaze-hw-v2");
    if let Some(machine_id_hash) = machine_id_hash.filter(|value| !value.trim().is_empty()) {
        hasher.update(machine_id_hash.as_bytes());
    } else {
        hasher.update(host_name.as_bytes());
        hasher.update(processor.as_bytes());
        hasher.update(format!("{ram_gb:.2}:{cpu_cores}:{os_version}").as_bytes());
    }
    hex::encode(hasher.finalize())
}

#[cfg(windows)]
fn machine_id_hash() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let cryptography = hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography").ok()?;
    let machine_guid: String = cryptography.get_value("MachineGuid").ok()?;
    let machine_guid = machine_guid.trim().to_ascii_lowercase();
    if machine_guid.is_empty() {
        return None;
    }

    Some(sha256_hex(
        format!("machine-guid:{machine_guid}").as_bytes(),
    ))
}

#[cfg(not(windows))]
fn machine_id_hash() -> Option<String> {
    None
}

fn sha256_hex(value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value);
    hex::encode(hasher.finalize())
}

fn bytes_to_mb(value: u64) -> f64 {
    value as f64 / 1024.0 / 1024.0
}

fn bytes_to_gb(value: u64) -> f64 {
    value as f64 / 1024.0 / 1024.0 / 1024.0
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn sane_temperature(value: f32) -> bool {
    value.is_finite() && (1.0..=125.0).contains(&value)
}

fn sane_temperature_f64(value: f64) -> bool {
    value.is_finite() && (1.0..=125.0).contains(&value)
}

#[cfg(windows)]
fn external_cpu_temperature_methods() -> Vec<CpuTemperatureMethod> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$lhm = Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Sensor |
  Where-Object { $_.SensorType -eq 'Temperature' -and ($_.Name -match 'CPU|Package|Core|Tctl|Tdie') } |
  Select-Object @{Name='source';Expression={'libre_hardware_monitor'}}, @{Name='label';Expression={$_.Name}}, @{Name='value_c';Expression={[double]$_.Value}}
$ohm = Get-CimInstance -Namespace root/OpenHardwareMonitor -ClassName Sensor |
  Where-Object { $_.SensorType -eq 'Temperature' -and ($_.Name -match 'CPU|Package|Core|Tctl|Tdie') } |
  Select-Object @{Name='source';Expression={'open_hardware_monitor'}}, @{Name='label';Expression={$_.Name}}, @{Name='value_c';Expression={[double]$_.Value}}
$acpi = Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature |
  Select-Object @{Name='source';Expression={'acpi_thermal_zone'}}, @{Name='label';Expression={$_.InstanceName}}, @{Name='value_c';Expression={([double]$_.CurrentTemperature / 10) - 273.15}}
@($lhm + $ohm + $acpi) | Where-Object { $_.value_c -gt 0 -and $_.value_c -lt 126 } | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output();

    let Ok(output) = output else {
        return vec![CpuTemperatureMethod::unavailable(
            "external_wmi",
            "PowerShell/CIM indisponivel",
        )];
    };

    if !output.status.success() {
        return vec![CpuTemperatureMethod::unavailable(
            "external_wmi",
            "fontes WMI indisponiveis",
        )];
    }

    parse_external_temperature_methods(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn external_cpu_temperature_methods() -> Vec<CpuTemperatureMethod> {
    vec![CpuTemperatureMethod::unavailable(
        "external_wmi",
        "fonte externa disponivel apenas no Windows",
    )]
}

#[derive(Debug, Deserialize)]
struct ExternalTemperatureMethod {
    source: Option<String>,
    label: Option<String>,
    value_c: Option<f64>,
}

fn parse_external_temperature_methods(raw: &str) -> Vec<CpuTemperatureMethod> {
    let raw = raw.trim();
    if raw.is_empty() {
        return vec![CpuTemperatureMethod::unavailable(
            "external_wmi",
            "nenhum sensor externo encontrado",
        )];
    }

    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => {
            return vec![CpuTemperatureMethod::unavailable(
                "external_wmi",
                "resposta WMI invalida",
            )]
        }
    };

    let entries: Vec<ExternalTemperatureMethod> = if value.is_array() {
        serde_json::from_value(value).unwrap_or_default()
    } else if value.is_object() {
        serde_json::from_value(value)
            .map(|entry| vec![entry])
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut methods = entries
        .into_iter()
        .filter_map(|entry| {
            let value_c = entry.value_c.filter(|value| sane_temperature_f64(*value))?;
            Some(CpuTemperatureMethod {
                source: entry.source.unwrap_or_else(|| "external_wmi".to_string()),
                label: entry.label.filter(|label| !label.trim().is_empty()),
                value_c: Some(value_c),
                available: true,
            })
        })
        .collect::<Vec<_>>();

    if methods.is_empty() {
        methods.push(CpuTemperatureMethod::unavailable(
            "external_wmi",
            "nenhum sensor externo valido",
        ));
    }

    methods
}

fn is_gpu_sensor_label(label: &str) -> bool {
    label.contains("gpu")
        || label.contains("nvidia")
        || label.contains("radeon")
        || label.contains("geforce")
        || label.contains("graphics")
}

fn is_cpu_sensor_label(label: &str) -> bool {
    label.contains("cpu")
        || label.contains("processor")
        || label.contains("package")
        || label.contains("core")
        || label.contains("tctl")
        || label.contains("tdie")
}

fn running_process_names(system: &System) -> Vec<String> {
    system
        .processes()
        .values()
        .filter_map(|process| {
            let name = process.name().to_string_lossy().trim().to_ascii_lowercase();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn detect_local_context(
    active_window: Option<&str>,
    process_names: &[String],
    gpu_usage: f64,
    cpu_usage: f64,
    idle_seconds: u64,
) -> Value {
    let active = active_window.unwrap_or_default().to_ascii_lowercase();
    let has_process = |needles: &[&str]| {
        process_names
            .iter()
            .any(|name| needles.iter().any(|needle| name.contains(needle)))
    };
    let active_has = |needles: &[&str]| needles.iter().any(|needle| active.contains(needle));

    let gaming_process = has_process(&[
        "steam.exe",
        "epicgameslauncher",
        "riotclient",
        "valorant",
        "cs2",
        "fortnite",
        "roblox",
        "minecraft",
        "battle.net",
        "leagueclient",
    ]);
    let gaming_window = active_has(&[
        "steam",
        "valorant",
        "counter-strike",
        "fortnite",
        "minecraft",
        "league of legends",
        "game",
    ]);
    let music = has_process(&["spotify", "musicbee", "itunes", "foobar", "deezer", "tidal"])
        || active_has(&["spotify", "deezer", "music", "youtube music"]);
    let video = has_process(&["vlc", "mpv", "netflix", "primevideo", "disney", "obs64"])
        || active_has(&[
            "youtube",
            "netflix",
            "prime video",
            "disney+",
            "twitch",
            "vlc",
        ]);
    let gaming = gaming_process
        || gaming_window
        || (gpu_usage >= 65.0 && cpu_usage >= 25.0 && idle_seconds < 120);
    let idle = idle_seconds >= 300;
    let activity = if idle {
        "idle"
    } else if gaming {
        "gaming"
    } else if video {
        "video"
    } else if music {
        "music"
    } else {
        "general"
    };

    json!({
        "activity": activity,
        "signals": {
            "gaming": gaming,
            "music": music,
            "video": video,
            "idle": idle,
        },
        "media": {
            "music": music,
            "video": video,
        },
        "active_window": active_window,
        "gpu_usage": gpu_usage,
        "cpu_usage": cpu_usage,
        "idle_seconds": idle_seconds,
    })
}

#[derive(Debug, Clone, Copy)]
struct NvidiaSensorSample {
    temperature_c: Option<f32>,
    utilization_percent: Option<f64>,
    vram_used_gb: Option<f64>,
}

struct NvidiaSensorReader {
    _library: libloading::Library,
    shutdown: unsafe extern "C" fn() -> u32,
    device_get_count: unsafe extern "C" fn(*mut u32) -> u32,
    device_get_handle_by_index: unsafe extern "C" fn(u32, *mut NvmlDevice) -> u32,
    device_get_temperature: unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> u32,
    device_get_utilization_rates: unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> u32,
    device_get_memory_info: unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> u32,
}

unsafe impl Send for NvidiaSensorReader {}
unsafe impl Sync for NvidiaSensorReader {}

impl NvidiaSensorReader {
    fn new() -> Option<Self> {
        unsafe {
            let library = libloading::Library::new("nvml.dll").ok()?;
            let init = *library
                .get::<unsafe extern "C" fn() -> u32>(b"nvmlInit_v2\0")
                .ok()?;
            let shutdown = *library
                .get::<unsafe extern "C" fn() -> u32>(b"nvmlShutdown\0")
                .ok()?;
            let device_get_count = *library
                .get::<unsafe extern "C" fn(*mut u32) -> u32>(b"nvmlDeviceGetCount_v2\0")
                .ok()?;
            let device_get_handle_by_index = *library
                .get::<unsafe extern "C" fn(u32, *mut NvmlDevice) -> u32>(
                    b"nvmlDeviceGetHandleByIndex_v2\0",
                )
                .ok()?;
            let device_get_temperature = *library
                .get::<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> u32>(
                    b"nvmlDeviceGetTemperature\0",
                )
                .ok()?;
            let device_get_utilization_rates = *library
                .get::<unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> u32>(
                    b"nvmlDeviceGetUtilizationRates\0",
                )
                .ok()?;
            let device_get_memory_info = *library
                .get::<unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> u32>(
                    b"nvmlDeviceGetMemoryInfo\0",
                )
                .ok()?;

            if init() != NVML_SUCCESS {
                return None;
            }

            Some(Self {
                _library: library,
                shutdown,
                device_get_count,
                device_get_handle_by_index,
                device_get_temperature,
                device_get_utilization_rates,
                device_get_memory_info,
            })
        }
    }

    fn sample(&self) -> Option<NvidiaSensorSample> {
        let mut device_count = 0;
        if unsafe { (self.device_get_count)(&mut device_count) } != NVML_SUCCESS {
            return None;
        }
        let mut best: Option<(u64, NvidiaSensorSample)> = None;

        for index in 0..device_count {
            let mut device = std::ptr::null_mut();
            if unsafe { (self.device_get_handle_by_index)(index, &mut device) } != NVML_SUCCESS
                || device.is_null()
            {
                continue;
            }

            let mut temperature = 0;
            let temperature_c = if unsafe {
                (self.device_get_temperature)(device, NVML_TEMPERATURE_GPU, &mut temperature)
            } == NVML_SUCCESS
            {
                Some(temperature as f32).filter(|value| sane_temperature(*value))
            } else {
                None
            };

            let mut utilization = NvmlUtilization::default();
            let utilization_percent =
                if unsafe { (self.device_get_utilization_rates)(device, &mut utilization) }
                    == NVML_SUCCESS
                {
                    Some(clamp_percent(utilization.gpu as f64))
                } else {
                    None
                };

            let mut memory = NvmlMemory::default();
            let memory_info =
                unsafe { (self.device_get_memory_info)(device, &mut memory) } == NVML_SUCCESS;
            let total_memory = if memory_info { memory.total } else { 0 };
            let sample = NvidiaSensorSample {
                temperature_c,
                utilization_percent,
                vram_used_gb: memory_info.then(|| bytes_to_gb(memory.used)),
            };

            if best
                .as_ref()
                .map(|(current_total, _)| total_memory > *current_total)
                .unwrap_or(true)
            {
                best = Some((total_memory, sample));
            }
        }

        best.map(|(_, sample)| sample)
    }
}

impl Drop for NvidiaSensorReader {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
        }
    }
}

type NvmlDevice = *mut std::ffi::c_void;

const NVML_SUCCESS: u32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[cfg(windows)]
fn widestring(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
struct GpuUsageReader {
    query: windows::Win32::System::Performance::PDH_HQUERY,
    counter: windows::Win32::System::Performance::PDH_HCOUNTER,
    has_baseline: bool,
}

#[cfg(windows)]
unsafe impl Send for GpuUsageReader {}

#[cfg(windows)]
impl GpuUsageReader {
    fn new() -> Option<Self> {
        use windows::core::PCWSTR;
        use windows::Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW, PDH_HCOUNTER,
            PDH_HQUERY,
        };

        let mut query = PDH_HQUERY::default();
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }

        let path = widestring(r"\GPU Engine(*)\Utilization Percentage");
        let mut counter = PDH_HCOUNTER::default();
        if unsafe { PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) } != 0 {
            unsafe {
                PdhCloseQuery(query);
            }
            return None;
        }

        unsafe {
            PdhCollectQueryData(query);
        }

        Some(Self {
            query,
            counter,
            has_baseline: false,
        })
    }

    fn sample(&mut self) -> Option<f64> {
        use windows::Win32::System::Performance::{
            PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
            PDH_FMT_DOUBLE, PDH_INVALID_DATA, PDH_MORE_DATA,
        };

        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }

        if !self.has_baseline {
            self.has_baseline = true;
            return None;
        }

        let mut buffer_size = 0;
        let mut item_count = 0;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                None,
            )
        };
        if status != PDH_MORE_DATA || buffer_size == 0 || item_count == 0 {
            return None;
        }

        let mut buffer = vec![0u8; buffer_size as usize];
        let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                Some(items),
            )
        };
        if status != 0 && status != PDH_INVALID_DATA {
            return None;
        }

        let values = unsafe { std::slice::from_raw_parts(items, item_count as usize) };
        let usage = values
            .iter()
            .filter_map(|item| {
                if item.FmtValue.CStatus == 0 {
                    Some(unsafe { item.FmtValue.Anonymous.doubleValue })
                } else {
                    None
                }
            })
            .filter(|value| value.is_finite() && *value > 0.0)
            .sum::<f64>();

        Some(clamp_percent(usage))
    }
}

#[cfg(windows)]
impl Drop for GpuUsageReader {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

#[cfg(not(windows))]
struct GpuUsageReader;

#[cfg(not(windows))]
impl GpuUsageReader {
    fn new() -> Option<Self> {
        None
    }

    fn sample(&mut self) -> Option<f64> {
        None
    }
}

#[cfg(windows)]
fn detect_gpu_devices() -> Vec<GpuInfo> {
    let dxgi_devices = dxgi_gpu_devices();
    if !dxgi_devices.is_empty() {
        return dxgi_devices;
    }

    registry_gpu_devices()
}

#[cfg(windows)]
fn dxgi_gpu_devices() -> Vec<GpuInfo> {
    use std::collections::HashSet;
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut devices = Vec::new();
    let mut index = 0;

    loop {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        index += 1;

        let Ok(description) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };

        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }

        let name = utf16_description(&description.Description);
        if !is_real_gpu_name(&name) || !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }

        devices.push(GpuInfo {
            name,
            vram_gb: bytes_to_gb(description.DedicatedVideoMemory as u64),
            vram_used_gb: adapter.cast::<IDXGIAdapter3>().ok().and_then(|adapter| {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                unsafe {
                    adapter
                        .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                        .ok()?;
                }
                Some(bytes_to_gb(info.CurrentUsage))
            }),
        });
    }

    devices
}

#[cfg(windows)]
fn utf16_description(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end]).trim().to_string()
}

#[cfg(windows)]
fn registry_gpu_devices() -> Vec<GpuInfo> {
    use std::collections::HashSet;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(video_root) = hklm.open_subkey("SYSTEM\\CurrentControlSet\\Control\\Video") else {
        return unknown_gpu();
    };

    let mut seen = HashSet::new();
    let mut devices = Vec::new();

    for adapter_key in video_root.enum_keys().flatten() {
        let Ok(adapter) = video_root.open_subkey(adapter_key) else {
            continue;
        };

        for child_key in adapter.enum_keys().flatten() {
            if !child_key.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }

            let Ok(device_key) = adapter.open_subkey(child_key) else {
                continue;
            };

            let Some(name) = registry_string(
                &device_key,
                &["DriverDesc", "HardwareInformation.AdapterString"],
            ) else {
                continue;
            };

            if !is_real_gpu_name(&name) || !seen.insert(name.to_ascii_lowercase()) {
                continue;
            }

            devices.push(GpuInfo {
                name,
                vram_gb: registry_vram_gb(&device_key).unwrap_or_default(),
                vram_used_gb: None,
            });
        }
    }

    if devices.is_empty() {
        unknown_gpu()
    } else {
        devices
    }
}

#[cfg(not(windows))]
fn detect_gpu_devices() -> Vec<GpuInfo> {
    unknown_gpu()
}

fn unknown_gpu() -> Vec<GpuInfo> {
    vec![GpuInfo {
        name: "Unknown GPU".to_string(),
        vram_gb: 0.0,
        vram_used_gb: None,
    }]
}

#[cfg(windows)]
fn registry_string(key: &winreg::RegKey, names: &[&str]) -> Option<String> {
    use winreg::enums::{REG_BINARY, REG_SZ};

    for name in names {
        if let Ok(value) = key.get_value::<String, _>(name) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }

        let Ok(raw) = key.get_raw_value(name) else {
            continue;
        };

        if raw.vtype == REG_BINARY || raw.vtype == REG_SZ {
            let utf16: Vec<u16> = raw
                .bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .take_while(|value| *value != 0)
                .collect();
            let value = String::from_utf16_lossy(&utf16).trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    None
}

#[cfg(windows)]
fn registry_vram_gb(key: &winreg::RegKey) -> Option<f64> {
    use winreg::enums::{REG_DWORD, REG_QWORD};

    let raw = key.get_raw_value("HardwareInformation.MemorySize").ok()?;
    let bytes = raw.bytes;
    let byte_count = match raw.vtype {
        REG_DWORD if bytes.len() >= 4 => {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
        }
        REG_QWORD if bytes.len() >= 8 => u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        _ => return None,
    };

    Some(bytes_to_gb(byte_count))
}

#[cfg(windows)]
fn is_real_gpu_name(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && !normalized.contains("basic render")
        && !normalized.contains("remote display")
        && !normalized.contains("mirror driver")
}

#[cfg(windows)]
fn active_window_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }

    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return None;
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if copied <= 0 {
        return None;
    }

    Some(
        String::from_utf16_lossy(&buffer[..copied as usize])
            .trim()
            .to_string(),
    )
    .filter(|title| !title.is_empty())
}

#[cfg(not(windows))]
fn active_window_title() -> Option<String> {
    None
}

#[cfg(windows)]
fn idle_seconds() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut last_input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    if !unsafe { GetLastInputInfo(&mut last_input) }.as_bool() {
        return 0;
    }

    let now_ms = unsafe { GetTickCount64() };
    now_ms.saturating_sub(last_input.dwTime as u64) / 1_000
}

#[cfg(not(windows))]
fn idle_seconds() -> u64 {
    0
}
