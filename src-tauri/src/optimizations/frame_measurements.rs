//! L4 (redesign D3): histórico local das medições de FPS antes/depois do
//! Modo Jogo automático.
//!
//! A medição em si nasce em `apply_game_mode` (20 s antes, 3 s de pausa,
//! 20 s depois) e já viaja para o servidor sem o nome do jogo. Este arquivo
//! guarda uma cópia no PC, com o nome do jogo, para a tela mostrar "1% low
//! de 156 para 240 fps no Valorant". O nome nunca sai daqui.

use serde_json::{json, Value};
use std::fs;
use std::sync::Mutex;

use super::snapshot;

/// Medições guardadas; as mais velhas saem primeiro.
const MAX_STORED: usize = 30;

static FILE_LOCK: Mutex<()> = Mutex::new(());

fn store_path() -> std::path::PathBuf {
    snapshot::app_data_dir().join("frame-measurements.json")
}

fn read_all() -> Vec<Value> {
    fs::read_to_string(store_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .unwrap_or_default()
}

/// Uma entrada do histórico. Pura, para ser testável.
pub fn build_entry(measured_at: i64, game_process_name: Option<&str>, measurement: &Value) -> Value {
    json!({
        "id": uuid::Uuid::new_v4().simple().to_string(),
        "measuredAt": measured_at,
        "gameProcessName": game_process_name,
        "measurement": measurement,
    })
}

/// Coloca a entrada nova na frente e corta o histórico em `MAX_STORED`.
pub fn push_entry(mut entries: Vec<Value>, entry: Value) -> Vec<Value> {
    entries.insert(0, entry);
    entries.truncate(MAX_STORED);
    entries
}

/// Grava uma medição (válida ou pulada, com o motivo) do Modo Jogo.
pub fn record(game_process_name: Option<&str>, measurement: &Value) -> Result<(), String> {
    if measurement.is_null() {
        return Ok(());
    }
    let _guard = FILE_LOCK.lock().map_err(|_| "lock".to_string())?;
    let entry = build_entry(chrono::Utc::now().timestamp(), game_process_name, measurement);
    let entries = push_entry(read_all(), entry);
    let text = serde_json::to_string(&entries).map_err(|error| error.to_string())?;
    let path = store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, text).map_err(|error| error.to_string())
}

/// As últimas medições, da mais nova para a mais velha.
pub fn recent(limit: usize) -> Vec<Value> {
    let _guard = FILE_LOCK.lock();
    read_all().into_iter().take(limit.clamp(1, MAX_STORED)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_capped() {
        let mut entries = Vec::new();
        for index in 0..(MAX_STORED + 5) {
            entries = push_entry(entries, json!({ "n": index }));
        }
        assert_eq!(entries.len(), MAX_STORED);
        assert_eq!(entries[0]["n"], json!(MAX_STORED + 4));
    }

    #[test]
    fn entry_keeps_the_game_name_and_the_measurement() {
        let measurement = json!({ "verdict": "better", "deltaLow1PctFps": 84.0 });
        let entry = build_entry(1_800_000_000, Some("VALORANT-Win64-Shipping.exe"), &measurement);
        assert_eq!(entry["gameProcessName"], json!("VALORANT-Win64-Shipping.exe"));
        assert_eq!(entry["measurement"]["verdict"], json!("better"));
        assert_eq!(entry["measuredAt"], json!(1_800_000_000_i64));
    }
}
