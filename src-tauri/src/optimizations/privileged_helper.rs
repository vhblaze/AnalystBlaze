use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PrivilegedHelperStatus {
    pub available: bool,
    pub installed: bool,
    pub version: Option<String>,
    pub can_request_uac: bool,
    pub supported_actions: Vec<String>,
    pub message: String,
}

pub fn status() -> PrivilegedHelperStatus {
    PrivilegedHelperStatus {
        available: false,
        installed: false,
        version: None,
        can_request_uac: false,
        supported_actions: vec![
            "CLEAR_STANDBY_LIST".to_string(),
            "STOP_SENSITIVE_SERVICE".to_string(),
            "WRITE_HKLM_REGISTRY".to_string(),
            "APPLY_ADVANCED_LATENCY_TWEAKS".to_string(),
        ],
        message: "Helper privilegiado ainda nao instalado. O agente principal continua rodando como usuario comum e bloqueia acoes admin.".to_string(),
    }
}
