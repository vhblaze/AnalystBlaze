use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use crate::optimizations::snapshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: i64,
    pub level: String,
    pub event: String,
    pub message: String,
    pub details: Value,
}

pub fn record_event(
    level: impl Into<String>,
    event: impl Into<String>,
    message: impl Into<String>,
    details: Value,
) -> Result<(), String> {
    let event = AuditEvent {
        timestamp: chrono::Utc::now().timestamp(),
        level: level.into(),
        event: event.into(),
        message: message.into(),
        details: sanitize_value(&details, 0),
    };

    let path = snapshot::app_data_dir().join("audit-log.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let line = serde_json::to_string(&event).map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())
}

pub fn recent_events(limit: usize) -> Result<Vec<AuditEvent>, String> {
    let path = snapshot::app_data_dir().join("audit-log.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let reader = BufReader::new(file);
    let mut events = reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<AuditEvent>(&line).ok())
        .collect::<Vec<_>>();

    events.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    events.truncate(limit.clamp(1, 250));
    Ok(events)
}

fn sanitize_value(value: &Value, depth: usize) -> Value {
    if depth >= 3 {
        return json!("[nested]");
    }

    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .take(40)
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let value = if normalized.contains("token")
                        || normalized.contains("secret")
                        || normalized.contains("password")
                        || normalized.contains("signature")
                        || normalized.contains("authorization")
                    {
                        json!("[redacted]")
                    } else {
                        sanitize_value(value, depth + 1)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(50)
                .map(|value| sanitize_value(value, depth + 1))
                .collect(),
        ),
        Value::String(value) => json!(value.chars().take(500).collect::<String>()),
        primitive => primitive.clone(),
    }
}
