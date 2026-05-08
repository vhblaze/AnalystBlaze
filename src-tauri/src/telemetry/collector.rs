use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sysinfo::{Components, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_gb: f64,
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
    pub gpu_usage: f64,
    pub gpu_name: String,
    pub vram_gb: f64,
    pub ram_usage_mb: f64,
    pub gpu_temperature: f64,
    pub latency_ms: f64,
    pub context_state: Value,
    pub details: Value,
}

pub struct TelemetryCollector {
    system: System,
    components: Components,
    gpu_devices: Vec<GpuInfo>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
            components: Components::new_with_refreshed_list(),
            gpu_devices: detect_gpu_devices(),
        }
    }

    pub fn collect(&mut self) -> TelemetrySample {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.components.refresh(false);

        let cpu_usage = self.system.global_cpu_usage() as f64;
        let ram_usage_mb = bytes_to_mb(self.system.used_memory());
        let gpu_temperature = self.detect_gpu_temperature().unwrap_or_default() as f64;
        let primary_gpu = self.primary_gpu();

        TelemetrySample {
            event_timestamp: chrono::Utc::now().timestamp(),
            cpu_usage: clamp_percent(cpu_usage),
            gpu_usage: 0.0,
            gpu_name: primary_gpu.name.clone(),
            vram_gb: primary_gpu.vram_gb,
            ram_usage_mb,
            gpu_temperature,
            latency_ms: 0.0,
            context_state: json!({
                "mode": "observed",
                "host_name": System::host_name().unwrap_or_else(|| "unknown".to_string()),
                "gpu_name": primary_gpu.name,
                "vram_gb": primary_gpu.vram_gb,
            }),
            details: json!({
                "gpu_name": self.primary_gpu().name,
                "vram_gb": self.primary_gpu().vram_gb,
                "gpu_devices": self.gpu_devices,
                "gpu_temperature": gpu_temperature,
                "latency_ms": 0.0,
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
            .find(|gpu| !gpu.name.eq_ignore_ascii_case("Unknown GPU"))
            .cloned()
            .unwrap_or_else(|| GpuInfo {
                name: "Unknown GPU".to_string(),
                vram_gb: 0.0,
            })
    }

    fn detect_gpu_temperature(&self) -> Option<f32> {
        self.components
            .iter()
            .find_map(|component| {
                let label = component.label().to_ascii_lowercase();
                if label.contains("gpu") {
                    component.temperature().filter(|value| value.is_finite())
                } else {
                    None
                }
            })
            .or_else(|| {
                self.components
                    .iter()
                    .filter_map(|component| component.temperature())
                    .filter(|value| value.is_finite())
                    .max_by(|left, right| left.total_cmp(right))
            })
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
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

#[cfg(windows)]
fn detect_gpu_devices() -> Vec<GpuInfo> {
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
