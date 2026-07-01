use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
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
    pub gpu_temperature_source: Option<String>,
    pub gpu_temperature_methods: Vec<CpuTemperatureMethod>,
    pub thermal_sensors: Vec<HardwareSensorReading>,
    pub power_sensors: Vec<HardwareSensorReading>,
    pub fan_sensors: Vec<HardwareSensorReading>,
    pub thermal_state: String,
    pub thermal_trend: String,
    pub throttling_suspected: bool,
    pub watts: Option<f64>,
    pub cpu_watts: Option<f64>,
    pub gpu_watts: Option<f64>,
    pub energy_confidence: f64,
    pub energy_is_estimated: bool,
    pub energy_source: String,
    pub power_profile: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSensorReading {
    pub source: String,
    pub sensor_type: String,
    pub hardware_type: Option<String>,
    pub hardware_name: Option<String>,
    pub identifier: Option<String>,
    pub label: Option<String>,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Default)]
struct CpuTemperatureReading {
    value_c: Option<f64>,
    source: Option<String>,
    methods: Vec<CpuTemperatureMethod>,
}

#[derive(Debug, Clone)]
struct EnergyEstimate {
    watts: Option<f64>,
    cpu_watts: Option<f64>,
    gpu_watts: Option<f64>,
    confidence: f64,
    is_estimated: bool,
    source: String,
    profile: String,
}

#[derive(Debug, Clone)]
struct ThermalAnalysis {
    state: String,
    trend: String,
    throttling_suspected: bool,
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
    hardware_sensor_cache: Vec<HardwareSensorReading>,
    hardware_sensor_refreshed_at: i64,
    thermal_history: VecDeque<(i64, Option<f64>, Option<f64>, f64)>,
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
            hardware_sensor_cache: Vec::new(),
            hardware_sensor_refreshed_at: 0,
            thermal_history: VecDeque::with_capacity(36),
        }
    }

    pub fn collect(&mut self) -> TelemetrySample {
        self.collection_count = self.collection_count.saturating_add(1);
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.components.refresh(false);
        if self.collection_count == 1 || self.collection_count.is_multiple_of(5) {
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
        let hardware_sensors = self.hardware_sensors();
        let thermal_sensors = limited_sensors_by_type(&hardware_sensors, "temperature", 32);
        let power_sensors = limited_sensors_by_type(&hardware_sensors, "power", 24);
        let fan_sensors = limited_sensors_by_type(&hardware_sensors, "fan", 16);
        let cpu_temperature_reading = self.detect_cpu_temperature(&hardware_sensors);
        let cpu_temperature_available = cpu_temperature_reading.value_c.is_some();
        let cpu_temperature = cpu_temperature_reading.value_c.unwrap_or_default();
        let cpu_temperature_source = cpu_temperature_reading.source.clone();
        let cpu_temperature_methods = cpu_temperature_reading.methods.clone();
        let nvidia_sensors = self
            .nvidia_sensor_reader
            .as_ref()
            .and_then(NvidiaSensorReader::sample);
        let mut gpu_temperature_methods = self.gpu_temperature_methods(&hardware_sensors);
        if let Some(temperature) = nvidia_sensors
            .as_ref()
            .and_then(|sample| sample.temperature_c)
        {
            gpu_temperature_methods.insert(
                0,
                CpuTemperatureMethod {
                    source: "nvml".to_string(),
                    label: Some("GPU core".to_string()),
                    value_c: Some(temperature as f64),
                    available: true,
                },
            );
        }
        let gpu_temperature_reading = CpuTemperatureReading::from_methods(gpu_temperature_methods);
        let gpu_temperature_available = gpu_temperature_reading.value_c.is_some();
        let gpu_temperature = gpu_temperature_reading.value_c.unwrap_or_default();
        let gpu_temperature_source = gpu_temperature_reading.source.clone();
        let gpu_temperature_methods = gpu_temperature_reading.methods.clone();
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
        let thermal_analysis = self.analyze_thermal(
            cpu_temperature_available.then_some(cpu_temperature),
            gpu_temperature_available.then_some(gpu_temperature),
            cpu_usage,
        );
        let energy = estimate_energy(EnergyEstimateInput {
            cpu_usage,
            gpu_usage: gpu_usage.unwrap_or_default(),
            gpu_usage_available: gpu_usage.is_some(),
            ram_usage_percent,
            disk_usage_percent,
            active_processes,
            cpu_temperature: cpu_temperature_available.then_some(cpu_temperature),
            gpu_temperature: gpu_temperature_available.then_some(gpu_temperature),
            advanced: &advanced,
            local_context: &local_context,
            hardware_sensors: &hardware_sensors,
        });

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
            gpu_temperature_source: gpu_temperature_source.clone(),
            gpu_temperature_methods: gpu_temperature_methods.clone(),
            thermal_sensors: thermal_sensors.clone(),
            power_sensors: power_sensors.clone(),
            fan_sensors: fan_sensors.clone(),
            thermal_state: thermal_analysis.state.clone(),
            thermal_trend: thermal_analysis.trend.clone(),
            throttling_suspected: thermal_analysis.throttling_suspected,
            watts: energy.watts,
            cpu_watts: energy.cpu_watts,
            gpu_watts: energy.gpu_watts,
            energy_confidence: energy.confidence,
            energy_is_estimated: energy.is_estimated,
            energy_source: energy.source.clone(),
            power_profile: energy.profile.clone(),
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
                "gpu_temperature_source": gpu_temperature_source.clone(),
                "gpu_temperature_methods": gpu_temperature_methods.clone(),
                "gpu_temperature_available": gpu_temperature_available,
                "thermal_sensors": thermal_sensors.clone(),
                "power_sensors": power_sensors.clone(),
                "fan_sensors": fan_sensors.clone(),
                "thermal_state": thermal_analysis.state,
                "thermal_trend": thermal_analysis.trend,
                "throttling_suspected": thermal_analysis.throttling_suspected,
                "watts": energy.watts,
                "cpu_watts": energy.cpu_watts,
                "gpu_watts": energy.gpu_watts,
                "estimated_kwh": energy.watts.map(|watts| watts / 1000.0),
                "energy_confidence": energy.confidence,
                "is_estimated": energy.is_estimated,
                "energy_source": energy.source.clone(),
                "power_profile": energy.profile.clone(),
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
                "gpu_temperature_source": gpu_temperature_source,
                "gpu_temperature_methods": gpu_temperature_methods,
                "thermal_sensors": thermal_sensors,
                "power_sensors": power_sensors,
                "fan_sensors": fan_sensors,
                "thermal_state": self
                    .thermal_history
                    .back()
                    .map(|_| self.last_thermal_state(cpu_temperature_available.then_some(cpu_temperature), gpu_temperature_available.then_some(gpu_temperature)))
                    .unwrap_or_else(|| "unknown".to_string()),
                "thermal_trend": self.thermal_trend(),
                "throttling_suspected": self.throttling_suspected(cpu_temperature_available.then_some(cpu_temperature), gpu_temperature_available.then_some(gpu_temperature), cpu_usage),
                "watts": energy.watts,
                "cpu_watts": energy.cpu_watts,
                "gpu_watts": energy.gpu_watts,
                "estimated_kwh": energy.watts.map(|watts| watts / 1000.0),
                "energy_confidence": energy.confidence,
                "is_estimated": energy.is_estimated,
                "energy_source": energy.source.clone(),
                "power_profile": energy.profile.clone(),
                "energy": {
                    "watts": energy.watts,
                    "cpuWatts": energy.cpu_watts,
                    "gpuWatts": energy.gpu_watts,
                    "estimatedKwh": energy.watts.map(|watts| watts / 1000.0),
                    "confidence": energy.confidence,
                    "isEstimated": energy.is_estimated,
                    "source": energy.source,
                    "profile": energy.profile,
                },
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

    fn analyze_thermal(
        &mut self,
        cpu_temperature: Option<f64>,
        gpu_temperature: Option<f64>,
        cpu_usage: f64,
    ) -> ThermalAnalysis {
        let now = chrono::Utc::now().timestamp();
        self.thermal_history
            .push_back((now, cpu_temperature, gpu_temperature, cpu_usage));
        while self.thermal_history.len() > 36 {
            let _ = self.thermal_history.pop_front();
        }

        ThermalAnalysis {
            state: self.last_thermal_state(cpu_temperature, gpu_temperature),
            trend: self.thermal_trend(),
            throttling_suspected: self.throttling_suspected(
                cpu_temperature,
                gpu_temperature,
                cpu_usage,
            ),
        }
    }

    fn last_thermal_state(
        &self,
        cpu_temperature: Option<f64>,
        gpu_temperature: Option<f64>,
    ) -> String {
        let max_temp = [cpu_temperature, gpu_temperature]
            .into_iter()
            .flatten()
            .fold(0.0_f64, f64::max);
        if max_temp <= 0.0 {
            "unknown".to_string()
        } else if max_temp >= 92.0 || gpu_temperature.is_some_and(|value| value >= 87.0) {
            "critical".to_string()
        } else if max_temp >= 84.0 || gpu_temperature.is_some_and(|value| value >= 80.0) {
            "hot".to_string()
        } else if max_temp >= 74.0 {
            "watch".to_string()
        } else {
            "normal".to_string()
        }
    }

    fn thermal_trend(&self) -> String {
        let Some(first) = self.thermal_history.front() else {
            return "unknown".to_string();
        };
        let Some(last) = self.thermal_history.back() else {
            return "unknown".to_string();
        };
        if last.0.saturating_sub(first.0) < 45 {
            return "warming_up".to_string();
        }
        let first_max = [first.1, first.2]
            .into_iter()
            .flatten()
            .fold(0.0_f64, f64::max);
        let last_max = [last.1, last.2]
            .into_iter()
            .flatten()
            .fold(0.0_f64, f64::max);
        if first_max <= 0.0 || last_max <= 0.0 {
            return "unknown".to_string();
        }
        let delta = last_max - first_max;
        if delta >= 4.0 {
            "rising".to_string()
        } else if delta <= -4.0 {
            "falling".to_string()
        } else {
            "stable".to_string()
        }
    }

    fn throttling_suspected(
        &self,
        cpu_temperature: Option<f64>,
        gpu_temperature: Option<f64>,
        cpu_usage: f64,
    ) -> bool {
        let thermal_limit = cpu_temperature.is_some_and(|value| value >= 92.0)
            || gpu_temperature.is_some_and(|value| value >= 87.0);
        if !thermal_limit {
            return false;
        }
        let recent_high_load = self
            .thermal_history
            .iter()
            .rev()
            .take(12)
            .any(|(_, _, _, usage)| *usage >= 75.0);
        recent_high_load && cpu_usage < 55.0
    }

    pub fn hardware_profile(&self, include_hostname: bool) -> HardwareProfile {
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
                "host_name": if include_hostname { json!(host_name) } else { Value::Null },
                "host_name_policy": if include_hostname { "included_by_local_opt_in" } else { "local_only" },
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

    fn gpu_temperature_methods(
        &self,
        hardware_sensors: &[HardwareSensorReading],
    ) -> Vec<CpuTemperatureMethod> {
        let mut methods = Vec::new();

        for component in self.components.iter() {
            let raw_label = component.label().trim().to_string();
            let label = raw_label.to_ascii_lowercase();
            if !is_gpu_sensor_label(&label) {
                continue;
            }
            let value = component
                .temperature()
                .filter(|value| sane_temperature(*value));
            methods.push(CpuTemperatureMethod {
                source: "sysinfo_gpu_sensor".to_string(),
                label: Some(raw_label).filter(|value| !value.is_empty()),
                value_c: value.map(|value| value as f64),
                available: value.is_some(),
            });
        }

        methods.extend(
            hardware_sensors
                .iter()
                .filter(|sensor| sensor.sensor_type == "temperature")
                .filter(|sensor| is_gpu_sensor_text(&sensor_search_text(sensor)))
                .map(sensor_temperature_method),
        );

        if methods.is_empty() {
            methods.push(CpuTemperatureMethod::unavailable(
                "gpu_temperature_sensor",
                "sensor GPU nao exposto",
            ));
        }

        methods
    }

    fn detect_cpu_temperature(
        &self,
        hardware_sensors: &[HardwareSensorReading],
    ) -> CpuTemperatureReading {
        let mut methods = self.sysinfo_cpu_temperature_methods();

        let mut external_methods = hardware_sensors
            .iter()
            .filter(|sensor| sensor.sensor_type == "temperature")
            .filter(|sensor| {
                let text = sensor_search_text(sensor);
                is_cpu_sensor_text(&text) || sensor.source == "acpi_thermal_zone"
            })
            .map(sensor_temperature_method)
            .collect::<Vec<_>>();

        if external_methods.is_empty() {
            methods.push(CpuTemperatureMethod::unavailable(
                "hardware_monitor",
                "sensor CPU externo nao exposto",
            ));
        } else {
            methods.append(&mut external_methods);
        }

        CpuTemperatureReading::from_methods(methods)
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

    fn hardware_sensors(&mut self) -> Vec<HardwareSensorReading> {
        let now = chrono::Utc::now().timestamp();
        if now.saturating_sub(self.hardware_sensor_refreshed_at) >= 30 {
            self.hardware_sensor_cache = external_hardware_sensors();
            self.hardware_sensor_refreshed_at = now;
        }

        self.hardware_sensor_cache.clone()
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

struct EnergyEstimateInput<'a> {
    cpu_usage: f64,
    gpu_usage: f64,
    gpu_usage_available: bool,
    ram_usage_percent: f64,
    disk_usage_percent: f64,
    active_processes: usize,
    cpu_temperature: Option<f64>,
    gpu_temperature: Option<f64>,
    advanced: &'a AdvancedTelemetry,
    local_context: &'a Value,
    hardware_sensors: &'a [HardwareSensorReading],
}

fn estimate_energy(input: EnergyEstimateInput<'_>) -> EnergyEstimate {
    let activity = input
        .local_context
        .get("activity")
        .and_then(Value::as_str)
        .unwrap_or("general");
    let on_battery = input
        .advanced
        .battery_status
        .as_deref()
        .is_some_and(|status| {
            let normalized = status.to_ascii_lowercase();
            normalized.contains("discharging") || normalized.contains("descarregando")
        });
    let has_battery = input.advanced.battery_percent.is_some();
    let profile = if activity == "gaming" {
        "gaming"
    } else if on_battery {
        "battery"
    } else if input.cpu_usage < 12.0 && input.gpu_usage < 8.0 {
        "idle"
    } else if input.cpu_usage >= 55.0 || input.gpu_usage >= 45.0 {
        "performance"
    } else {
        "balanced"
    };

    let base_watts = if on_battery {
        14.0
    } else if has_battery {
        22.0
    } else {
        42.0
    };
    let estimated_cpu_watts =
        (8.0 + input.cpu_usage * if on_battery { 0.42 } else { 0.70 }).clamp(6.0, 105.0);
    let estimated_gpu_watts = if input.gpu_usage_available {
        Some(
            (if activity == "gaming" { 18.0 } else { 5.0 } + input.gpu_usage * 1.45)
                .clamp(0.0, 240.0),
        )
    } else {
        None
    };
    let sensor_cpu_watts = best_power_sensor(input.hardware_sensors, PowerSensorTarget::Cpu);
    let sensor_gpu_watts = best_power_sensor(input.hardware_sensors, PowerSensorTarget::Gpu);
    let sensor_system_watts = best_power_sensor(input.hardware_sensors, PowerSensorTarget::System);
    let cpu_watts = sensor_cpu_watts.unwrap_or(estimated_cpu_watts);
    let gpu_watts = sensor_gpu_watts.or(estimated_gpu_watts);
    let memory_watts = (input.ram_usage_percent * 0.12).clamp(0.0, 14.0);
    let disk_watts = (input.disk_usage_percent * 0.04).clamp(1.0, 8.0);
    let process_watts = ((input.active_processes as f64 / 80.0).clamp(0.0, 6.0)).round();
    let thermal_penalty = input
        .cpu_temperature
        .map(|value| ((value - 82.0).max(0.0) * 0.5).clamp(0.0, 12.0))
        .unwrap_or_default()
        + input
            .gpu_temperature
            .map(|value| ((value - 78.0).max(0.0) * 0.45).clamp(0.0, 12.0))
            .unwrap_or_default();
    let activity_extra = match profile {
        "gaming" => 22.0,
        "performance" => 10.0,
        "battery" => -4.0,
        "idle" => -6.0,
        _ => 0.0,
    };
    let mut watts = sensor_system_watts.unwrap_or_else(|| {
        base_watts
            + cpu_watts
            + gpu_watts.unwrap_or_default()
            + memory_watts
            + disk_watts
            + process_watts
            + thermal_penalty
            + activity_extra
    });
    watts = if on_battery {
        watts.clamp(12.0, 180.0)
    } else {
        watts.clamp(28.0, 520.0)
    };

    let sensor_component_count =
        sensor_cpu_watts.is_some() as u8 + sensor_gpu_watts.is_some() as u8;
    let confidence = if sensor_system_watts.is_some() {
        0.90
    } else if sensor_component_count == 2 {
        0.84
    } else if sensor_component_count == 1 {
        0.76
    } else if input.gpu_usage_available
        && (input.cpu_temperature.is_some() || input.gpu_temperature.is_some())
    {
        0.68
    } else if input.gpu_usage_available {
        0.58
    } else {
        0.46
    };
    let source = if sensor_system_watts.is_some() {
        "system_power_sensor"
    } else if sensor_component_count > 0 {
        "component_power_sensors"
    } else {
        "hybrid_estimate"
    };

    EnergyEstimate {
        watts: Some(round1(watts)),
        cpu_watts: Some(round1(cpu_watts)),
        gpu_watts: gpu_watts.map(round1),
        confidence,
        is_estimated: sensor_system_watts.is_none(),
        source: source.to_string(),
        profile: profile.to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
enum PowerSensorTarget {
    Cpu,
    Gpu,
    System,
}

fn best_power_sensor(sensors: &[HardwareSensorReading], target: PowerSensorTarget) -> Option<f64> {
    sensors
        .iter()
        .filter(|sensor| sensor.sensor_type == "power")
        .filter(|sensor| {
            let text = sensor_search_text(sensor);
            match target {
                PowerSensorTarget::Cpu => {
                    is_cpu_sensor_text(&text)
                        || text.contains("package power")
                        || text.contains("ppt")
                }
                PowerSensorTarget::Gpu => {
                    is_gpu_sensor_text(&text)
                        || text.contains("total board power")
                        || text.contains("graphics power")
                }
                PowerSensorTarget::System => {
                    !is_cpu_sensor_text(&text)
                        && !is_gpu_sensor_text(&text)
                        && (text.contains("system power")
                            || text.contains("total system")
                            || text.contains("wall power")
                            || text.contains("battery discharge"))
                }
            }
        })
        .map(|sensor| sensor.value)
        .filter(|value| sane_sensor_value("power", *value) && *value > 0.1)
        .max_by(f64::total_cmp)
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

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn sane_temperature(value: f32) -> bool {
    value.is_finite() && (1.0..=125.0).contains(&value)
}

fn sane_temperature_f64(value: f64) -> bool {
    value.is_finite() && (1.0..=125.0).contains(&value)
}

#[cfg(windows)]
fn external_hardware_sensors() -> Vec<HardwareSensorReading> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
function Unit-ForSensorType([string]$type) {
  switch -Regex ($type) {
    'Temperature' { 'C'; break }
    'Power' { 'W'; break }
    'Fan' { 'RPM'; break }
    'Voltage' { 'V'; break }
    'Load' { '%'; break }
    default { '' }
  }
}
function Read-MonitorSensors([string]$namespace, [string]$source) {
  Get-CimInstance -Namespace $namespace -ClassName Sensor -ErrorAction SilentlyContinue |
    Where-Object { $_.Value -ne $null -and $_.SensorType -in @('Temperature','Power','Fan','Voltage','Load') } |
    ForEach-Object {
      [pscustomobject]@{
        source = $source
        sensor_type = [string]$_.SensorType
        hardware_type = [string]$_.HardwareType
        hardware_name = [string]$_.HardwareName
        identifier = [string]$_.Identifier
        label = [string]$_.Name
        value = [double]$_.Value
        unit = Unit-ForSensorType ([string]$_.SensorType)
      }
    }
}
$lhm = Read-MonitorSensors 'root/LibreHardwareMonitor' 'libre_hardware_monitor'
$ohm = Read-MonitorSensors 'root/OpenHardwareMonitor' 'open_hardware_monitor'
$acpi = Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature |
  ForEach-Object {
    [pscustomobject]@{
      source = 'acpi_thermal_zone'
      sensor_type = 'Temperature'
      hardware_type = 'ACPI'
      hardware_name = [string]$_.InstanceName
      identifier = [string]$_.InstanceName
      label = [string]$_.InstanceName
      value = ([double]$_.CurrentTemperature / 10) - 273.15
      unit = 'C'
    }
  }
@($lhm + $ohm + $acpi) | ConvertTo-Json -Compress
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
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    parse_external_hardware_sensors(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn external_hardware_sensors() -> Vec<HardwareSensorReading> {
    Vec::new()
}

#[derive(Debug, Deserialize)]
struct ExternalHardwareSensor {
    source: Option<String>,
    sensor_type: Option<String>,
    hardware_type: Option<String>,
    hardware_name: Option<String>,
    identifier: Option<String>,
    label: Option<String>,
    value: Option<f64>,
    unit: Option<String>,
}

fn parse_external_hardware_sensors(raw: &str) -> Vec<HardwareSensorReading> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }

    let value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let entries: Vec<ExternalHardwareSensor> = if value.is_array() {
        serde_json::from_value(value).unwrap_or_default()
    } else if value.is_object() {
        serde_json::from_value(value)
            .map(|entry| vec![entry])
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    entries
        .into_iter()
        .filter_map(|entry| {
            let sensor_type = normalize_sensor_type(entry.sensor_type.as_deref()?);
            let value = entry
                .value
                .filter(|value| sane_sensor_value(&sensor_type, *value))?;
            Some(HardwareSensorReading {
                source: clean_sensor_string(entry.source.as_deref())
                    .unwrap_or_else(|| "external_hardware_monitor".to_string()),
                sensor_type: sensor_type.clone(),
                hardware_type: clean_sensor_string(entry.hardware_type.as_deref()),
                hardware_name: clean_sensor_string(entry.hardware_name.as_deref()),
                identifier: clean_sensor_string(entry.identifier.as_deref()),
                label: clean_sensor_string(entry.label.as_deref()),
                value: round_sensor_value(value),
                unit: clean_sensor_string(entry.unit.as_deref())
                    .unwrap_or_else(|| default_sensor_unit(&sensor_type).to_string()),
            })
        })
        .take(96)
        .collect()
}

fn limited_sensors_by_type(
    sensors: &[HardwareSensorReading],
    sensor_type: &str,
    limit: usize,
) -> Vec<HardwareSensorReading> {
    sensors
        .iter()
        .filter(|sensor| sensor.sensor_type == sensor_type)
        .take(limit)
        .cloned()
        .collect()
}

fn sensor_temperature_method(sensor: &HardwareSensorReading) -> CpuTemperatureMethod {
    CpuTemperatureMethod {
        source: sensor.source.clone(),
        label: sensor
            .label
            .clone()
            .or_else(|| sensor.hardware_name.clone())
            .or_else(|| sensor.identifier.clone()),
        value_c: Some(sensor.value).filter(|value| sane_temperature_f64(*value)),
        available: sane_temperature_f64(sensor.value),
    }
}

fn normalize_sensor_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "temperature" | "temperatura" => "temperature",
        "power" | "potencia" | "watt" | "watts" => "power",
        "fan" | "fans" | "rpm" => "fan",
        "voltage" | "volt" | "volts" => "voltage",
        "load" | "usage" | "utilization" => "load",
        _ => "unknown",
    }
    .to_string()
}

fn default_sensor_unit(sensor_type: &str) -> &'static str {
    match sensor_type {
        "temperature" => "C",
        "power" => "W",
        "fan" => "RPM",
        "voltage" => "V",
        "load" => "%",
        _ => "",
    }
}

fn sane_sensor_value(sensor_type: &str, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }

    match sensor_type {
        "temperature" => sane_temperature_f64(value),
        "power" => (0.0..=1500.0).contains(&value),
        "fan" => (0.0..=20_000.0).contains(&value),
        "voltage" => (0.0..=64.0).contains(&value),
        "load" => (0.0..=100.0).contains(&value),
        _ => false,
    }
}

fn round_sensor_value(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn clean_sensor_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(160).collect::<String>())
}

fn sensor_search_text(sensor: &HardwareSensorReading) -> String {
    [
        sensor.hardware_type.as_deref(),
        sensor.hardware_name.as_deref(),
        sensor.identifier.as_deref(),
        sensor.label.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

fn is_cpu_sensor_text(text: &str) -> bool {
    is_cpu_sensor_label(text) && !is_gpu_sensor_label(text)
}

fn is_gpu_sensor_text(text: &str) -> bool {
    is_gpu_sensor_label(text)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_external_hardware_monitor_sensors() {
        let raw = r#"[
            {
                "source":"libre_hardware_monitor",
                "sensor_type":"Temperature",
                "hardware_type":"Cpu",
                "hardware_name":"AMD Ryzen",
                "identifier":"/amdcpu/0/temperature/2",
                "label":"CPU Package",
                "value":64.25,
                "unit":"C"
            },
            {
                "source":"libre_hardware_monitor",
                "sensor_type":"Power",
                "hardware_type":"GpuNvidia",
                "hardware_name":"NVIDIA GeForce",
                "identifier":"/gpu-nvidia/0/power/0",
                "label":"GPU Package",
                "value":142.8,
                "unit":"W"
            }
        ]"#;

        let sensors = parse_external_hardware_sensors(raw);

        assert_eq!(sensors.len(), 2);
        assert_eq!(sensors[0].sensor_type, "temperature");
        assert_eq!(sensors[0].unit, "C");
        assert_eq!(sensors[1].sensor_type, "power");
        assert_eq!(sensors[1].unit, "W");
    }

    #[test]
    fn selects_component_power_sensors() {
        let sensors = vec![
            HardwareSensorReading {
                source: "libre_hardware_monitor".to_string(),
                sensor_type: "power".to_string(),
                hardware_type: Some("Cpu".to_string()),
                hardware_name: Some("AMD Ryzen".to_string()),
                identifier: None,
                label: Some("CPU Package".to_string()),
                value: 72.0,
                unit: "W".to_string(),
            },
            HardwareSensorReading {
                source: "libre_hardware_monitor".to_string(),
                sensor_type: "power".to_string(),
                hardware_type: Some("GpuNvidia".to_string()),
                hardware_name: Some("NVIDIA GeForce".to_string()),
                identifier: None,
                label: Some("GPU Total Board Power".to_string()),
                value: 156.0,
                unit: "W".to_string(),
            },
        ];

        assert_eq!(
            best_power_sensor(&sensors, PowerSensorTarget::Cpu),
            Some(72.0)
        );
        assert_eq!(
            best_power_sensor(&sensors, PowerSensorTarget::Gpu),
            Some(156.0)
        );
    }
}
