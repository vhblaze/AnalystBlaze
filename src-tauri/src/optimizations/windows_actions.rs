use serde_json::{json, Value};
use std::process::Command;

use super::{
    safety,
    snapshot::{self, OptimizationSnapshot, SnapshotEntry},
    windows_inventory, ExecutionResult,
};
use crate::process_ext::{decode_console_bytes, CommandExt};

pub async fn disable_startup_app(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(payload.as_ref(), &["target", "name", "app", "value_name"]);
    let location = extract_payload_string(payload.as_ref(), &["location", "registry_location"]);
    let fallback_payload = payload.clone();

    let Some(target) = target else {
        return ExecutionResult {
            success: false,
            message: "Informe o nome do app de inicializacao.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || {
        disable_startup_app_sync(&target, location.as_deref())
    })
    .await
    {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao desativar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

pub async fn restore_startup_app(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(payload.as_ref(), &["target", "name", "app", "value_name"]);
    let fallback_payload = payload.clone();

    match tokio::task::spawn_blocking(move || {
        snapshot::restore_startup_app_snapshots(target.as_deref())
    })
    .await
    {
        Ok(Ok(report)) => {
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 {
                    "Nenhum snapshot de app de inicializacao pendente para restaurar.".to_string()
                } else if success {
                    "App(s) de inicializacao restaurado(s) por snapshot local.".to_string()
                } else {
                    "Restauracao de app de inicializacao concluida com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "payload": fallback_payload,
                    "restore": report,
                }),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar app de inicializacao: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

/// Services worth pausing for the duration of a game session - each one is
/// non-critical background work that competes for CPU/disk/RAM (SysMain in
/// particular actively works AGAINST optimizations::memory's standby-list
/// clear, since its whole job is preloading apps back into RAM). Kept
/// short and conservative on purpose: `safety::is_critical_service` already
/// refuses anything actually load-bearing, but the point here isn't "stop
/// everything non-critical" (that's a much longer, riskier list - Windows
/// Update, print spooler, etc.) - it's the small set of well-known,
/// widely-recommended "safe to pause while gaming" services.
const GAME_MODE_PAUSABLE_SERVICES: &[&str] = &[
    "SysMain", // Superfetch - preloads apps into RAM based on usage patterns
    "WSearch", // Windows Search - background file indexing
];

/// Stops whichever of GAME_MODE_PAUSABLE_SERVICES are actually running,
/// each through the same snapshot-backed stop_service_sync used by the
/// standalone STOP_SERVICE action - so restoring the Modo Gamer session
/// (either path: process-exit monitor or manual restore) already knows how
/// to bring them back via the existing SnapshotEntry::ServiceState restore,
/// no new restore logic needed. Returns the combined step result (for the
/// same steps.* reporting shape apply_game_mode already builds) and the
/// flat list of snapshot ids to fold into the session's snapshot_ids.
pub async fn stop_nonessential_services_for_game_mode() -> (ExecutionResult, Vec<String>) {
    let results: Vec<(String, ExecutionResult)> =
        stop_each_sequentially(GAME_MODE_PAUSABLE_SERVICES).await;

    let snapshot_ids: Vec<String> = results
        .iter()
        .filter_map(|(_, result)| {
            result
                .details
                .pointer("/snapshot/id")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect();
    let stopped: Vec<&str> = results
        .iter()
        .filter(|(_, result)| result.success && result.details.get("changed") == Some(&json!(true)))
        .map(|(name, _)| name.as_str())
        .collect();

    let summary = ExecutionResult {
        // Never the reason Modo Gamer itself reports failure - a service
        // that's already stopped, missing, or access-denied just means one
        // less thing paused, not a broken activation.
        success: true,
        message: if stopped.is_empty() {
            "Nenhum servico adicional precisou ser pausado.".to_string()
        } else {
            format!("Servicos pausados durante o jogo: {}.", stopped.join(", "))
        },
        details: json!({
            "implemented": true,
            "stopped": stopped,
            "attempted": GAME_MODE_PAUSABLE_SERVICES,
            "results": results
                .iter()
                .map(|(name, result)| json!({
                    "service": name,
                    "success": result.success,
                    "message": result.message,
                }))
                .collect::<Vec<_>>(),
        }),
    };

    (summary, snapshot_ids)
}

async fn stop_each_sequentially(services: &[&str]) -> Vec<(String, ExecutionResult)> {
    let mut results = Vec::with_capacity(services.len());
    for service in services {
        let result = stop_service(Some(json!({ "service": service }))).await;
        results.push(((*service).to_string(), result));
    }
    results
}

/// Apps well-known enough, and lightweight enough, to close and relaunch
/// without losing anything a user would notice - communication/media apps
/// with no unsaved-document concept of their own, matched by exact process
/// image name only. Deliberately an ALLOWLIST, not "anything not on some
/// protected-apps denylist": protected_apps.rs's list was built to guard
/// DISABLE_STARTUP_APP (security tools, VPNs, drivers), never vetted as
/// "safe to force-close a running instance of" - a browser mid-video-call,
/// an editor with an unsaved tab, or the game itself would all pass a
/// denylist check while being exactly the wrong things to close. Growing
/// this list is a one-line addition once a candidate has actually been
/// reviewed, not something to do speculatively.
///
/// Real-time audio/voice processing tools (Voicemod and anything like it)
/// deliberately do NOT belong on this list, even with the active-use check
/// in place - found the hard way in the field: silence between spoken
/// words is normal for a mic pipe someone is actively using for a call, not
/// evidence they're done with it, and Core Audio's "active session" signal
/// isn't a reliable enough proxy for "this tool is mid-use" the way it is
/// for something like Spotify. And unlike Spotify, relaunching one mid-game
/// often means reconfiguring it (reselecting the voice, reconnecting it as
/// the game/Discord's input device) rather than just picking up where it
/// left off - a real disruption, not a minor one.
const GAME_MODE_CLOSABLE_APPS: &[&str] = &[
    "Discord.exe",
    "Spotify.exe",
    "WhatsApp.Root.exe",
    "NVIDIA Overlay.exe",
    "Skype.exe",
    "Telegram.exe",
    // Microsoft's own gaming-companion overlay (Xbox Game Bar-adjacent,
    // ships as part of Edge) - same category as NVIDIA Overlay: a vendor
    // overlay helper, not something holding user state.
    "EdgeGameAssist.exe",
];

/// Closes whichever of GAME_MODE_CLOSABLE_APPS are running and NOT in
/// active use right now (see active_use.rs: not the foreground window, no
/// live audio session on either the speaker or microphone side) - a
/// process someone's actively talking through, listening to, or looking
/// at is skipped even if it's on the list. Unlike stop_nonessential_services,
/// this has no snapshot/restore: closing an app isn't a toggle to undo,
/// it's the same one-way action as EMPTY_TEMP - the app relaunches itself
/// next time the user opens it, same as if they'd closed it by hand.
pub async fn close_nonessential_apps_for_game_mode() -> ExecutionResult {
    match tokio::task::spawn_blocking(close_nonessential_apps_for_game_mode_sync).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao fechar apps nao essenciais: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

/// Groups every running process matching a GAME_MODE_CLOSABLE_APPS name by
/// that name, then decides per GROUP (never per pid) whether it's in active
/// use - see close_nonessential_apps_for_game_mode_sync's doc comment for
/// why per-pid checking is the wrong granularity for a multi-process app.
/// Pure and OS-call-free on purpose, so the actual grouping/skip decision -
/// the part that was wrong before - has a real regression test instead of
/// only "it compiles and looks right".
fn group_closable_processes_by_name<N: AsRef<str>>(
    processes: impl IntoIterator<Item = (u32, N)>,
) -> std::collections::HashMap<&'static str, Vec<u32>> {
    let mut candidates: std::collections::HashMap<&'static str, Vec<u32>> =
        std::collections::HashMap::new();
    for (pid, name) in processes {
        if let Some(&matched) = GAME_MODE_CLOSABLE_APPS
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name.as_ref()))
        {
            candidates.entry(matched).or_default().push(pid);
        }
    }
    candidates
}

/// Splits grouped candidates into (names safe to close, names skipped
/// because at least one of their pids is in active use). Still pure: takes
/// the active-use signals as plain data rather than calling
/// active_use::foreground_process_id()/processes_with_active_audio_session()
/// itself.
fn partition_closable_groups<'a>(
    candidates: &std::collections::HashMap<&'a str, Vec<u32>>,
    foreground_pid: Option<u32>,
    active_audio_pids: &std::collections::HashSet<u32>,
) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut closable = Vec::new();
    let mut skipped_active_use = Vec::new();
    for (&name, pids) in candidates {
        let any_pid_active = pids.iter().any(|&pid| {
            super::active_use::is_in_active_use(pid, foreground_pid, active_audio_pids)
        });
        if any_pid_active {
            skipped_active_use.push(name);
        } else {
            closable.push(name);
        }
    }
    closable.sort_unstable();
    skipped_active_use.sort_unstable();
    (closable, skipped_active_use)
}

fn close_nonessential_apps_for_game_mode_sync() -> ExecutionResult {
    use sysinfo::{ProcessesToUpdate, System};

    let foreground_pid = super::active_use::foreground_process_id();
    let active_audio_pids = super::active_use::processes_with_active_audio_session();

    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let candidates = group_closable_processes_by_name(
        system
            .processes()
            .iter()
            .map(|(pid, process)| (pid.as_u32(), process.name().to_string_lossy())),
    );

    let (closable_names, skipped_active_use) =
        partition_closable_groups(&candidates, foreground_pid, &active_audio_pids);

    let mut closed = Vec::new();
    for name in closable_names {
        let Some(pids) = candidates.get(name) else {
            continue;
        };
        let mut any_killed = false;
        for &pid in pids {
            if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
                any_killed |= process.kill();
            }
        }
        if any_killed {
            closed.push(name);
        }
    }

    ExecutionResult::ok(
        // Same "never the reason Modo Gamer reports failure" rule as
        // stop_nonessential_services_for_game_mode - nothing to close, or
        // everything eligible being in active use, is a normal outcome.
        if closed.is_empty() {
            "Nenhum app nao essencial precisou ser fechado.".to_string()
        } else {
            format!("Apps fechados durante o jogo: {}.", closed.join(", "))
        },
        json!({
            "implemented": true,
            "closed": closed,
            "skipped_active_use": skipped_active_use,
            "candidates": GAME_MODE_CLOSABLE_APPS,
        }),
    )
}

pub async fn stop_service(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(
        payload.as_ref(),
        &["target", "service", "service_name", "name"],
    );
    let fallback_payload = payload.clone();

    let Some(service_name) = target else {
        return ExecutionResult {
            success: false,
            message: "Informe o nome do servico do Windows.".to_string(),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        };
    };

    match tokio::task::spawn_blocking(move || stop_service_sync(&service_name)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao parar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

pub async fn restore_service(payload: Option<Value>) -> ExecutionResult {
    let target = extract_payload_string(
        payload.as_ref(),
        &["target", "service", "service_name", "name"],
    );
    let fallback_payload = payload.clone();

    match tokio::task::spawn_blocking(move || {
        snapshot::restore_service_snapshots(target.as_deref())
    })
    .await
    {
        Ok(Ok(report)) => {
            let success = report.failed_snapshots == 0 && report.failed_entries == 0;
            ExecutionResult {
                success,
                message: if report.restored_snapshots == 0 {
                    "Nenhum snapshot de servico pendente para restaurar.".to_string()
                } else if success {
                    "Servico(s) restaurado(s) por snapshot local.".to_string()
                } else {
                    "Restauracao de servico concluida com falhas.".to_string()
                },
                details: json!({
                    "implemented": true,
                    "payload": fallback_payload,
                    "restore": report,
                }),
            }
        }
        Ok(Err(error)) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao restaurar servico: {error}"),
            details: json!({ "implemented": true, "payload": fallback_payload }),
        },
    }
}

#[cfg(windows)]
fn disable_startup_app_sync(target: &str, location: Option<&str>) -> ExecutionResult {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let Some((hive, subkey)) = resolve_startup_location(target, location) else {
        return ExecutionResult {
            success: false,
            message: "App de inicializacao nao encontrado no inventario local.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "location": location,
            }),
        };
    };

    if !is_allowed_startup_subkey(&subkey) {
        return ExecutionResult {
            success: false,
            message: "Local de inicializacao nao permitido pela camada local.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "hive": hive,
                "subkey": subkey,
            }),
        };
    }

    let root = match hive.as_str() {
        "HKCU" => RegKey::predef(HKEY_CURRENT_USER),
        "HKLM" => RegKey::predef(HKEY_LOCAL_MACHINE),
        _ => {
            return ExecutionResult {
                success: false,
                message: "Hive de registro nao suportada.".to_string(),
                details: json!({ "implemented": true, "hive": hive }),
            }
        }
    };

    let key = match root.open_subkey_with_flags(&subkey, KEY_READ | KEY_WRITE) {
        Ok(key) => key,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel abrir a chave de inicializacao para escrita."
                    .to_string(),
                details: json!({
                    "implemented": true,
                    "target": target,
                    "hive": hive,
                    "subkey": subkey,
                    "requires_admin": hive == "HKLM",
                    "error": error.to_string(),
                }),
            }
        }
    };

    let raw_value = match key.get_raw_value(target) {
        Ok(value) => value,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Valor de inicializacao nao encontrado.".to_string(),
                details: json!({
                    "implemented": true,
                    "target": target,
                    "hive": hive,
                    "subkey": subkey,
                    "error": error.to_string(),
                }),
            }
        }
    };

    let snapshot = OptimizationSnapshot::new(
        "DISABLE_STARTUP_APP",
        vec![SnapshotEntry::StartupRegistryValue {
            hive: hive.clone(),
            subkey: subkey.clone(),
            value_name: target.to_string(),
            value_type: format!("{:?}", raw_value.vtype),
            value_bytes: raw_value.bytes.clone(),
        }],
        json!({
            "target": target,
            "hive": hive,
            "subkey": subkey,
            "value_type": format!("{:?}", raw_value.vtype),
            "command_preview": raw_value.to_string(),
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "snapshot_error": error,
            }),
        };
    }

    if let Err(error) = key.delete_value(target) {
        let _ = snapshot::discard_snapshot(&snapshot.id);
        return ExecutionResult {
            success: false,
            message: "Nao foi possivel remover o app da inicializacao.".to_string(),
            details: json!({
                "implemented": true,
                "target": target,
                "snapshot_discarded": true,
                "error": error.to_string(),
            }),
        };
    }

    ExecutionResult::ok(
        "App removido da inicializacao com snapshot reversivel.",
        json!({
            "implemented": true,
            "target": target,
            "hive": hive,
            "subkey": subkey,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
                "reversible": true,
            },
        }),
    )
}

#[cfg(not(windows))]
fn disable_startup_app_sync(target: &str, location: Option<&str>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Apps de inicializacao do Windows indisponiveis nesta plataforma.".to_string(),
        details: json!({
            "implemented": true,
            "target": target,
            "location": location,
        }),
    }
}

#[cfg(windows)]
fn stop_service_sync(service_name: &str) -> ExecutionResult {
    if safety::is_critical_service(service_name) {
        return ExecutionResult {
            success: false,
            message: "Servico critico protegido pela denylist local.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "blocked_by": "critical_service_denylist",
            }),
        };
    }

    let state = match query_service_state(service_name) {
        Ok(state) => state,
        Err(error) => {
            return ExecutionResult {
                success: false,
                message: "Servico nao encontrado ou inacessivel.".to_string(),
                details: json!({
                    "implemented": true,
                    "service": service_name,
                    "error": error,
                }),
            }
        }
    };

    if !state.running {
        return ExecutionResult::ok(
            "Servico ja estava parado; nenhuma alteracao aplicada.",
            json!({
                "implemented": true,
                "service": service_name,
                "changed": false,
                "snapshot": null,
            }),
        );
    }

    let snapshot = OptimizationSnapshot::new(
        "STOP_SERVICE",
        vec![SnapshotEntry::ServiceState {
            service_name: service_name.to_string(),
            display_name: state.display_name.clone(),
            was_running: state.running,
            start_type: state.start_type,
        }],
        json!({
            "service": service_name,
            "display_name": state.display_name,
            "was_running": state.running,
            "start_type": state.start_type,
        }),
    );

    if let Err(error) = snapshot::save_snapshot(&snapshot) {
        return ExecutionResult {
            success: false,
            message: "A alteracao foi bloqueada porque o snapshot nao pode ser salvo.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "snapshot_error": error,
            }),
        };
    }

    let output = match Command::new("sc.exe").args(["stop", service_name]).no_window().output() {
        Ok(output) => output,
        Err(error) => {
            let _ = snapshot::discard_snapshot(&snapshot.id);
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel chamar o Service Control Manager.".to_string(),
                details: json!({
                    "implemented": true,
                    "service": service_name,
                    "snapshot_discarded": true,
                    "error": error.to_string(),
                }),
            };
        }
    };

    let stdout = decode_console_bytes(&output.stdout);
    let stderr = decode_console_bytes(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let accepted = output.status.success()
        || combined.contains("STOP_PENDING")
        || combined.contains("STOPPED")
        || combined.to_ascii_lowercase().contains("already stopped");

    if !accepted {
        let _ = snapshot::discard_snapshot(&snapshot.id);
        return ExecutionResult {
            success: false,
            message: "O Windows recusou parar o servico.".to_string(),
            details: json!({
                "implemented": true,
                "service": service_name,
                "snapshot_discarded": true,
                "requires_admin": access_denied(&combined),
                "stdout": stdout.trim(),
                "stderr": stderr.trim(),
            }),
        };
    }

    ExecutionResult::ok(
        "Servico parado com snapshot reversivel.",
        json!({
            "implemented": true,
            "service": service_name,
            "changed": true,
            "snapshot": {
                "id": snapshot.id,
                "entries": snapshot.entries.len(),
                "reversible": true,
            },
            "stdout": stdout.trim(),
        }),
    )
}

#[cfg(not(windows))]
fn stop_service_sync(service_name: &str) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Servicos do Windows indisponiveis nesta plataforma.".to_string(),
        details: json!({ "implemented": true, "service": service_name }),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct ServiceState {
    running: bool,
    display_name: Option<String>,
    start_type: Option<u32>,
}

#[cfg(windows)]
fn query_service_state(service_name: &str) -> Result<ServiceState, String> {
    let output = Command::new("sc.exe")
        .args(["query", service_name])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(decode_console_bytes(&output.stderr).trim().to_string());
    }

    let stdout = decode_console_bytes(&output.stdout);
    let running = stdout.contains("RUNNING")
        || stdout.contains("START_PENDING")
        || stdout.contains("PAUSED")
        || stdout.contains("PAUSE_PENDING")
        || stdout.contains("CONTINUE_PENDING");

    let inventory_match = windows_inventory::collect_windows_inventory()
        .services
        .into_iter()
        .find(|service| service.name.eq_ignore_ascii_case(service_name));

    Ok(ServiceState {
        running,
        display_name: inventory_match
            .as_ref()
            .and_then(|service| service.display_name.clone()),
        start_type: inventory_match.and_then(|service| service.start_type),
    })
}

#[cfg(windows)]
fn resolve_startup_location(target: &str, location: Option<&str>) -> Option<(String, String)> {
    if let Some(location) = location.and_then(parse_startup_location) {
        return Some(location);
    }

    windows_inventory::collect_windows_inventory()
        .startup_apps
        .into_iter()
        .find(|app| app.name.eq_ignore_ascii_case(target))
        .and_then(|app| parse_startup_location(&app.location))
}

#[cfg(windows)]
fn parse_startup_location(location: &str) -> Option<(String, String)> {
    let normalized = location.trim().replace('/', "\\");
    let (hive, subkey) = normalized.split_once('\\')?;
    let hive = match hive.to_ascii_uppercase().as_str() {
        "HKCU" | "HKEY_CURRENT_USER" => "HKCU",
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKLM",
        _ => return None,
    };
    Some((hive.to_string(), subkey.trim_matches('\\').to_string()))
}

#[cfg(windows)]
fn is_allowed_startup_subkey(subkey: &str) -> bool {
    matches!(
        subkey.to_ascii_lowercase().as_str(),
        "software\\microsoft\\windows\\currentversion\\run"
            | "software\\microsoft\\windows\\currentversion\\runonce"
    )
}

#[cfg(windows)]
fn access_denied(output: &str) -> bool {
    let normalized = output.to_ascii_lowercase();
    normalized.contains("access is denied")
        || normalized.contains("acesso negado")
        || normalized.contains("error 5")
        || normalized.contains("erro 5")
}

fn extract_payload_string(payload: Option<&Value>, keys: &[&str]) -> Option<String> {
    let payload = payload?;
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod closable_apps_tests {
    use super::{group_closable_processes_by_name, partition_closable_groups};
    use std::collections::HashSet;

    // Regression test for a real bug found in the field: Discord runs
    // several distinct "Discord.exe" processes (main, renderer, GPU,
    // utility), and only the one actually carrying a voice call's audio
    // stream showed up as "active" - checking active-use per PID instead of
    // per app closed a Discord process out from under a live call, even
    // though a *different* Discord pid was correctly flagged as active.
    #[test]
    fn a_multi_process_app_is_fully_protected_if_any_of_its_pids_is_in_a_call() {
        // Three real-shaped Discord pids: one owns the foreground window,
        // one is the audio-active voice-call process, one is an idle
        // renderer with neither signal - none should be considered
        // closable, because they all share the "Discord.exe" identity.
        let processes = [
            (100u32, "Discord.exe"),
            (101u32, "Discord.exe"),
            (102u32, "Discord.exe"),
            (200u32, "Spotify.exe"),
        ];
        let candidates = group_closable_processes_by_name(processes.into_iter());
        assert_eq!(candidates.get("Discord.exe").map(Vec::len), Some(3));

        let foreground_pid = Some(100u32);
        let active_audio_pids: HashSet<u32> = [101u32].into_iter().collect();

        let (closable, skipped) =
            partition_closable_groups(&candidates, foreground_pid, &active_audio_pids);

        assert!(
            !closable.contains(&"Discord.exe"),
            "Discord has an active pid (101, the voice call) - none of its \
             processes should be closable, not just pid 101 itself"
        );
        assert!(skipped.contains(&"Discord.exe"));
        assert!(
            closable.contains(&"Spotify.exe"),
            "Spotify has no active pid at all, so it should remain closable"
        );
    }

    #[test]
    fn an_app_with_no_active_signal_at_all_is_closable() {
        let processes = [(300u32, "Telegram.exe")];
        let candidates = group_closable_processes_by_name(processes.into_iter());
        let (closable, skipped) = partition_closable_groups(&candidates, None, &HashSet::new());

        assert_eq!(closable, vec!["Telegram.exe"]);
        assert!(skipped.is_empty());
    }

    #[test]
    fn matching_is_case_insensitive_and_ignores_unrelated_processes() {
        let processes = [
            (1u32, "DISCORD.EXE"),
            (2u32, "explorer.exe"),
            (3u32, "svchost.exe"),
        ];
        let candidates = group_closable_processes_by_name(processes.into_iter());

        assert_eq!(candidates.get("Discord.exe").map(Vec::len), Some(1));
        assert_eq!(candidates.len(), 1, "only the Discord match should be grouped at all");
    }

    #[test]
    fn foreground_window_alone_protects_every_pid_of_that_app() {
        let processes = [(10u32, "WhatsApp.Root.exe"), (11u32, "WhatsApp.Root.exe")];
        let candidates = group_closable_processes_by_name(processes.into_iter());

        // pid 11 owns the foreground window, not 10 - both still protected.
        let (closable, skipped) =
            partition_closable_groups(&candidates, Some(11), &HashSet::new());

        assert!(closable.is_empty());
        assert_eq!(skipped, vec!["WhatsApp.Root.exe"]);
    }
}
