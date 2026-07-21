use serde_json::{json, Value};

use super::{
    os_version::WindowsGeneration,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    ExecutionResult,
};
use crate::process_ext::CommandExt;

#[derive(Clone)]
struct VisualEffectSetting {
    label: &'static str,
    subkey: &'static str,
    value_name: &'static str,
    target: RegistryTargetValue,
    /// Restricts this setting to a single Windows generation. `None` means
    /// it applies to both. Windows 11's taskbar was rewritten from the
    /// ground up (Fluent design, centered icons, no thumbnail Aero Peek),
    /// so a couple of classic Explorer/DWM taskbar keys are widely reported
    /// as silent no-ops there - the registry write "succeeds" but nothing
    /// visually changes. We skip writing (and reporting on) those instead
    /// of claiming an effect that isn't real.
    applies_to: Option<WindowsGeneration>,
}

#[derive(Clone)]
enum RegistryTargetValue {
    Dword(u32),
    Sz(&'static str),
}

pub async fn apply_visual_performance_mode(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || apply_visual_performance_mode_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao aplicar modo visual de desempenho: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

pub async fn restore_visual_effects(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(snapshot::restore_visual_effect_snapshots).await {
        Ok(Ok(report)) => {
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 {
                    "Nenhum snapshot visual pendente para restaurar.".to_string()
                } else if success {
                    "Efeitos visuais do Windows restaurados por snapshot local.".to_string()
                } else {
                    "Restauracao visual concluida com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "payload": payload,
                    "restore": report,
                }),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar efeitos visuais: {error}"),
            details: json!({ "implemented": true, "payload": payload }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar efeitos visuais: {error}"),
            details: json!({ "implemented": true, "payload": payload }),
        },
    }
}

pub fn current_visual_effects_summary() -> Value {
    current_visual_effects_summary_sync()
}

fn performance_settings(payload: Option<&Value>) -> Vec<VisualEffectSetting> {
    filter_settings_for_generation(
        performance_settings_unfiltered(payload),
        super::os_version::detected().generation,
    )
}

fn performance_settings_unfiltered(payload: Option<&Value>) -> Vec<VisualEffectSetting> {
    let disable_drag_full_windows = payload_bool(payload, "disable_drag_full_windows", false);
    let mut settings = vec![
        VisualEffectSetting {
            label: "Perfil de efeitos visuais",
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects",
            value_name: "VisualFXSetting",
            target: RegistryTargetValue::Dword(2),
            applies_to: None,
        },
        VisualEffectSetting {
            label: "Animacao da barra de tarefas",
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            value_name: "TaskbarAnimations",
            target: RegistryTargetValue::Dword(0),
            applies_to: Some(WindowsGeneration::Windows10),
        },
        VisualEffectSetting {
            label: "Transparencia",
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            value_name: "EnableTransparency",
            target: RegistryTargetValue::Dword(0),
            applies_to: None,
        },
        VisualEffectSetting {
            label: "Aero Peek",
            subkey: r"Software\Microsoft\Windows\DWM",
            value_name: "EnableAeroPeek",
            target: RegistryTargetValue::Dword(0),
            applies_to: Some(WindowsGeneration::Windows10),
        },
        VisualEffectSetting {
            label: "Selecao translucida no Explorer",
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            value_name: "ListviewAlphaSelect",
            target: RegistryTargetValue::Dword(0),
            applies_to: None,
        },
        VisualEffectSetting {
            label: "Sombra de nomes no Explorer",
            subkey: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            value_name: "ListviewShadow",
            target: RegistryTargetValue::Dword(0),
            applies_to: None,
        },
        VisualEffectSetting {
            label: "Animacao de minimizar/maximizar",
            subkey: r"Control Panel\Desktop\WindowMetrics",
            value_name: "MinAnimate",
            target: RegistryTargetValue::Sz("0"),
            applies_to: None,
        },
        VisualEffectSetting {
            label: "Atraso de menus",
            subkey: r"Control Panel\Desktop",
            value_name: "MenuShowDelay",
            target: RegistryTargetValue::Sz("0"),
            applies_to: None,
        },
    ];

    if disable_drag_full_windows {
        settings.push(VisualEffectSetting {
            label: "Arrastar janelas completas",
            subkey: r"Control Panel\Desktop",
            value_name: "DragFullWindows",
            target: RegistryTargetValue::Sz("0"),
            applies_to: None,
        });
    }

    settings
}

fn filter_settings_for_generation(
    mut settings: Vec<VisualEffectSetting>,
    generation: WindowsGeneration,
) -> Vec<VisualEffectSetting> {
    settings.retain(|setting| setting.applies_to.is_none_or(|required| required == generation));
    settings
}

fn payload_bool(payload: Option<&Value>, key: &str, default: bool) -> bool {
    payload
        .and_then(|payload| payload.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

#[cfg(windows)]
fn apply_visual_performance_mode_sync(payload: Option<Value>) -> ExecutionResult {
    let before = current_visual_effects_summary_sync();
    let settings = performance_settings(payload.as_ref());
    let mut entries = Vec::new();
    let mut changed_settings = Vec::new();
    let mut failed_settings = Vec::new();

    for setting in settings {
        let target_value = target_to_reg_value(&setting.target);
        match read_hkcu_raw_value(setting.subkey, setting.value_name) {
            Ok(previous_value) => {
                if previous_value
                    .as_ref()
                    .is_some_and(|previous| same_registry_value(previous, &target_value))
                {
                    continue;
                }

                entries.push(SnapshotEntry::RegistryValue {
                    hive: "HKCU".to_string(),
                    subkey: setting.subkey.to_string(),
                    value_name: setting.value_name.to_string(),
                    previous_value_type: previous_value.as_ref().map(registry_type_name),
                    previous_value_bytes: previous_value.as_ref().map(|value| value.bytes.clone()),
                    target_value_type: registry_type_name(&target_value),
                    target_value_bytes: target_value.bytes.clone(),
                });
                changed_settings.push(json!({
                    "label": setting.label,
                    "subkey": setting.subkey,
                    "valueName": setting.value_name,
                    "previous": previous_value.as_ref().map(registry_value_json).unwrap_or(Value::Null),
                    "target": registry_value_json(&target_value),
                }));

                if let Err(error) =
                    write_hkcu_raw_value(setting.subkey, setting.value_name, &target_value)
                {
                    failed_settings.push(json!({
                        "label": setting.label,
                        "subkey": setting.subkey,
                        "valueName": setting.value_name,
                        "error": error,
                    }));
                }
            }
            Err(error) => failed_settings.push(json!({
                "label": setting.label,
                "subkey": setting.subkey,
                "valueName": setting.value_name,
                "error": error,
            })),
        }
    }

    if !failed_settings.is_empty() {
        let rollback = OptimizationSnapshot::new(
            "APPLY_VISUAL_PERFORMANCE_MODE",
            entries.clone(),
            json!({ "rollback_after_partial_failure": true }),
        );
        let rollback = snapshot::restore_snapshot_entries(&rollback);
        return ExecutionResult {
            success: false,
            message:
                "Alguns efeitos visuais nao puderam ser ajustados; reversao parcial solicitada."
                    .to_string(),
            details: json!({
                "implemented": true,
                "profile": "visual_performance",
                "changed_settings": changed_settings,
                "failed_settings": failed_settings,
                "rollback": {
                    "restored_entries": rollback.restored_entries,
                    "failed_entries": rollback.failed_entries,
                    "messages": rollback.messages,
                },
                "verification": {
                    "before": before,
                    "after": current_visual_effects_summary_sync(),
                },
            }),
        };
    }

    if entries.is_empty() {
        let after = current_visual_effects_summary_sync();
        return ExecutionResult {
            success: true,
            message: "Efeitos visuais do Windows ja estavam focados em desempenho.".to_string(),
            details: json!({
                "implemented": true,
                "profile": "visual_performance",
                "changed_settings": [],
                "snapshot": null,
                "verification": {
                    "before": before,
                    "after": after,
                },
            }),
        };
    }

    let snapshot = OptimizationSnapshot::new(
        "APPLY_VISUAL_PERFORMANCE_MODE",
        entries,
        json!({
            "profile": "visual_performance",
            "changed_settings": changed_settings,
            "reversible": true,
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        let rollback = snapshot::restore_snapshot_entries(&snapshot);
        return ExecutionResult {
            success: false,
            message: "Efeitos visuais revertidos porque o snapshot local falhou.".to_string(),
            details: json!({
                "implemented": true,
                "profile": "visual_performance",
                "snapshot_error": error,
                "rollback": {
                    "restored_entries": rollback.restored_entries,
                    "failed_entries": rollback.failed_entries,
                    "messages": rollback.messages,
                },
            }),
        };
    }

    let notification = notify_windows_settings_changed();
    let after = current_visual_effects_summary_sync();
    ExecutionResult {
        success: true,
        message: "Efeitos visuais ajustados para priorizar desempenho.".to_string(),
        details: json!({
            "implemented": true,
            "profile": "visual_performance",
            "changed_settings": changed_settings,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
            },
            "verification": {
                "before": before,
                "after": after,
            },
            "notification": notification,
            "restart_hint": "Alguns efeitos do Explorer podem exigir reinicio do Explorer, logoff ou reabertura de janelas para refletir visualmente.",
        }),
    }
}

#[cfg(not(windows))]
fn apply_visual_performance_mode_sync(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Efeitos visuais do Windows estao disponiveis apenas no Windows.".to_string(),
        details: json!({ "implemented": true, "payload": payload }),
    }
}

#[cfg(windows)]
fn current_visual_effects_summary_sync() -> Value {
    let settings = performance_settings(None)
        .into_iter()
        .map(|setting| {
            let value = read_hkcu_raw_value(setting.subkey, setting.value_name)
                .ok()
                .flatten();
            json!({
                "label": setting.label,
                "subkey": setting.subkey,
                "valueName": setting.value_name,
                "value": value.as_ref().map(registry_value_json).unwrap_or(Value::Null),
                "performanceTarget": registry_value_json(&target_to_reg_value(&setting.target)),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "source": "registry_hkcu",
        "settings": settings,
    })
}

#[cfg(not(windows))]
fn current_visual_effects_summary_sync() -> Value {
    json!({
        "source": "unavailable",
        "settings": [],
    })
}

#[cfg(windows)]
fn target_to_reg_value(target: &RegistryTargetValue) -> winreg::RegValue {
    use winreg::enums::{REG_DWORD, REG_SZ};

    match target {
        RegistryTargetValue::Dword(value) => winreg::RegValue {
            vtype: REG_DWORD,
            bytes: value.to_le_bytes().to_vec(),
        },
        RegistryTargetValue::Sz(value) => winreg::RegValue {
            vtype: REG_SZ,
            bytes: encode_reg_sz(value),
        },
    }
}

#[cfg(windows)]
fn same_registry_value(left: &winreg::RegValue, right: &winreg::RegValue) -> bool {
    left.vtype == right.vtype && left.bytes == right.bytes
}

#[cfg(windows)]
fn read_hkcu_raw_value(subkey: &str, value_name: &str) -> Result<Option<winreg::RegValue>, String> {
    use std::io;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key = match root.open_subkey_with_flags(subkey, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };

    match key.get_raw_value(value_name) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(windows)]
fn write_hkcu_raw_value(
    subkey: &str,
    value_name: &str,
    value: &winreg::RegValue,
) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = root
        .create_subkey(subkey)
        .map_err(|error| error.to_string())?;
    key.set_raw_value(value_name, value)
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn registry_type_name(value: &winreg::RegValue) -> String {
    format!("{:?}", value.vtype)
}

#[cfg(windows)]
fn registry_value_json(value: &winreg::RegValue) -> Value {
    use winreg::enums::{REG_DWORD, REG_SZ};

    if value.vtype == REG_DWORD && value.bytes.len() >= 4 {
        let number = u32::from_le_bytes([
            value.bytes[0],
            value.bytes[1],
            value.bytes[2],
            value.bytes[3],
        ]);
        return json!({
            "type": registry_type_name(value),
            "value": number,
        });
    }

    if value.vtype == REG_SZ {
        return json!({
            "type": registry_type_name(value),
            "value": decode_reg_sz(&value.bytes),
        });
    }

    json!({
        "type": registry_type_name(value),
        "bytes": value.bytes.clone(),
    })
}

#[cfg(windows)]
fn notify_windows_settings_changed() -> Value {
    let update_per_user = std::process::Command::new("rundll32.exe")
        .args(["user32.dll,UpdatePerUserSystemParameters"])
        .no_window()
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let explorer_refresh = std::process::Command::new("ie4uinit.exe")
        .arg("-show")
        .no_window()
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    json!({
        "updatePerUserSystemParameters": update_per_user,
        "explorerIconRefresh": explorer_refresh,
    })
}

#[cfg(windows)]
fn encode_reg_sz(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn decode_reg_sz(bytes: &[u8]) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::{decode_reg_sz, encode_reg_sz};
    use super::{filter_settings_for_generation, performance_settings_unfiltered};
    use crate::optimizations::os_version::WindowsGeneration;

    #[cfg(windows)]
    #[test]
    fn encodes_and_decodes_reg_sz() {
        let encoded = encode_reg_sz("0");
        assert_eq!(decode_reg_sz(&encoded), "0");
        assert_eq!(encoded, vec![48, 0, 0, 0]);
    }

    #[test]
    fn windows_11_drops_the_known_inert_taskbar_keys() {
        let all = performance_settings_unfiltered(None);
        let filtered = filter_settings_for_generation(all, WindowsGeneration::Windows11);
        assert!(!filtered.iter().any(|setting| setting.value_name == "TaskbarAnimations"));
        assert!(!filtered.iter().any(|setting| setting.value_name == "EnableAeroPeek"));
        // Non-taskbar settings still apply on Windows 11.
        assert!(filtered.iter().any(|setting| setting.value_name == "VisualFXSetting"));
        assert!(filtered.iter().any(|setting| setting.value_name == "ListviewAlphaSelect"));
    }

    #[test]
    fn windows_10_keeps_every_setting() {
        let all = performance_settings_unfiltered(None);
        let expected_count = all.len();
        let filtered = filter_settings_for_generation(all, WindowsGeneration::Windows10);
        assert_eq!(filtered.len(), expected_count);
    }

    #[test]
    fn unknown_generation_only_keeps_generation_agnostic_settings() {
        let all = performance_settings_unfiltered(None);
        let filtered = filter_settings_for_generation(all, WindowsGeneration::Unknown);
        assert!(filtered.iter().all(|setting| setting.applies_to.is_none()));
        assert!(!filtered.iter().any(|setting| setting.value_name == "TaskbarAnimations"));
    }
}
