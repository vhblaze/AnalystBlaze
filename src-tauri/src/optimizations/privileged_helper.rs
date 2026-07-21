use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::process_ext::CommandExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{safety, snapshot, ExecutionResult};
use crate::audit;

const SERVICE_NAME: &str = "AnalystBlazeHelper";
const SERVICE_DISPLAY_NAME: &str = "AnalystBlaze Privileged Helper";
const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");
const COMMAND_POLL_MS: u64 = 700;
const HELPER_PIPE_NAME: &str = r"\\.\pipe\AnalystBlazeHelperRpcV1";
const MAX_PIPE_FRAME_BYTES: usize = 1024 * 1024;
const REQUEST_PROTOCOL_VERSION: u32 = 1;
const REQUEST_ORIGIN: &str = "analystblaze-desktop-app";
// Reserved protocol health-check action. It never runs an optimization: the
// service answers a signed pong so the UI can prove the HMAC named-pipe channel
// works end to end ("Testar conexao"). Kept off the optimization allowlist on
// purpose and short-circuited before any safety validation.
const HELPER_PING_ACTION: &str = "HELPER_PING";
const REQUEST_TTL_SECONDS: i64 = 60;
const REQUEST_MAX_CLOCK_SKEW_SECONDS: i64 = 15;
const MIN_SIGNING_KEY_LEN: usize = 32;

type HmacSha256 = Hmac<Sha256>;

static SEEN_REQUEST_NONCES: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegedHelperStatus {
    pub available: bool,
    pub installed: bool,
    pub running: bool,
    pub version: Option<String>,
    pub requires_update: bool,
    pub can_request_uac: bool,
    pub supported_actions: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegedHelperHandshake {
    pub ok: bool,
    pub latency_ms: u64,
    pub helper_version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperCommandRequest {
    protocol_version: u32,
    origin: String,
    action_id: String,
    action_name: String,
    source: safety::CommandSource,
    local_confirmation: bool,
    payload: Option<Value>,
    payload_sha256: String,
    nonce: String,
    created_at: i64,
    expires_at: i64,
    signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperCommandResponse {
    action_id: String,
    request_nonce: String,
    success: bool,
    message: String,
    details: Value,
    finished_at: i64,
    signature: String,
}

#[derive(Debug, Clone)]
struct VerifiedPipeClient {
    pid: u32,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct RejectedHelperRequest {
    action_id: String,
    request_nonce: String,
    message: String,
}

pub fn status() -> PrivilegedHelperStatus {
    let installed = service_installed();
    let running = service_running();
    let service_source_trusted = !installed || installed_service_source_is_trusted();
    let request_signing_ready = !installed || helper_signing_key_is_readable();
    let available = installed && running && service_source_trusted && request_signing_ready;
    let requires_update = installed
        && helper_version_file()
            .as_deref()
            .is_some_and(|version| version != HELPER_VERSION);
    let install_source_trusted =
        current_exe_is_trusted_service_source() && current_exe_has_trusted_signature();
    PrivilegedHelperStatus {
        available,
        installed,
        running,
        version: helper_version_file(),
        requires_update,
        can_request_uac: cfg!(windows) && install_source_trusted,
        supported_actions: supported_actions()
            .iter()
            .map(|action| (*action).to_string())
            .collect(),
        message: if available && requires_update {
            format!(
                "Helper privilegiado desatualizado (versao {} instalada, app na versao {}). Reinicie o helper para sincronizar as versoes antes de acoes admin.",
                helper_version_file().unwrap_or_else(|| "desconhecida".to_string()),
                HELPER_VERSION,
            )
        } else if available {
            "Helper privilegiado instalado e rodando. Acoes admin exigem named pipe local autenticado, request assinado, nonce, expiracao curta, confirmacao local e allowlist.".to_string()
        } else if installed && !service_source_trusted {
            "Helper admin instalado de caminho inseguro. Remova o servico como Administrador e reinstale pelo instalador per-machine/Program Files.".to_string()
        } else if installed && !request_signing_ready {
            "Helper admin instalado, mas a chave local de assinatura nao esta disponivel. Reinicie ou reinstale o helper pelo instalador per-machine/Program Files.".to_string()
        } else if installed {
            "Helper privilegiado instalado, mas parado. Reinicie o helper para executar acoes admin sem novo UAC.".to_string()
        } else if cfg!(windows) && !install_source_trusted {
            "Helper admin bloqueado neste ambiente: instale o AnalystBlaze em modo per-machine/Program Files para criar o servico com seguranca.".to_string()
        } else {
            "Helper privilegiado ainda nao instalado. A instalacao pede UAC uma vez e cria um servico local.".to_string()
        },
    }
}

pub fn install() -> Result<PrivilegedHelperStatus, String> {
    #[cfg(not(windows))]
    {
        Err("Helper privilegiado esta disponivel apenas no Windows.".to_string())
    }

    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        if !exe_path_is_trusted_service_source(&exe) {
            return Err(format!(
                "Instalacao do helper bloqueada por seguranca. O servico admin nao pode apontar para este executavel: {}. Instale o AnalystBlaze em modo per-machine/Program Files e tente novamente.",
                exe.display()
            ));
        }
        if !exe_signature_is_trusted(&exe) {
            return Err(format!(
                "Instalacao do helper bloqueada por seguranca. O executavel nao possui assinatura Authenticode valida: {}.",
                exe.display()
            ));
        }
        let user_sid = current_user_sid()?;
        let script = elevated_script_path("install");
        let helper_root = helper_root();
        let script_body = format!(
            r#"
$ErrorActionPreference = 'Stop'
$helperRoot = '{helper_root}'
$appUserSid = '{user_sid}'
$systemFull = '*S-1-5-18:(OI)(CI)F'
$adminsFull = '*S-1-5-32-544:(OI)(CI)F'
$userRead = "*{{0}}:(OI)(CI)RX" -f $appUserSid
New-Item -ItemType Directory -Force -Path $helperRoot | Out-Null
Set-Content -Path (Join-Path $helperRoot 'version.txt') -Value '{version}' -Encoding UTF8
$keyPath = Join-Path $helperRoot 'request-signing.key'
if (!(Test-Path $keyPath)) {{
  $bytes = New-Object byte[] 32
  $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
  try {{
    $rng.GetBytes($bytes)
  }} finally {{
    $rng.Dispose()
  }}
  [Convert]::ToBase64String($bytes) | Set-Content -Path $keyPath -Encoding ASCII
}}
icacls $helperRoot /inheritance:r /grant:r $systemFull $adminsFull $userRead | Out-Null
icacls (Join-Path $helperRoot 'version.txt') /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' ("*{{0}}:R" -f $appUserSid) | Out-Null
icacls $keyPath /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' ("*{{0}}:R" -f $appUserSid) | Out-Null
$svc = Get-Service -Name '{service_name}' -ErrorAction SilentlyContinue
if ($svc) {{
  sc.exe stop '{service_name}' | Out-Null
  Start-Sleep -Milliseconds 800
  sc.exe delete '{service_name}' | Out-Null
  Start-Sleep -Milliseconds 800
}}
sc.exe create '{service_name}' binPath= '"{exe}" --analystblaze-helper-service' start= auto DisplayName= '{display_name}' | Out-Null
sc.exe description '{service_name}' 'Executes AnalystBlaze local privileged actions after app-side confirmation.' | Out-Null
sc.exe start '{service_name}' | Out-Null
"#,
            helper_root = ps_escape(&helper_root.display().to_string()),
            user_sid = ps_escape(&user_sid),
            version = HELPER_VERSION,
            service_name = SERVICE_NAME,
            display_name = SERVICE_DISPLAY_NAME,
            exe = ps_escape(&exe.display().to_string()),
        );
        fs::write(&script, script_body).map_err(|error| error.to_string())?;
        run_elevated_script(&script).inspect_err(|error| {
            audit_helper_event(
                "warn",
                "optimization.helper.install_failed",
                "Instalacao do helper privilegiado falhou.",
                json!({ "error": error }),
            );
        })?;
        audit_helper_event(
            "info",
            "optimization.helper.installed",
            "Helper privilegiado instalado ou atualizado.",
            json!({ "service_name": SERVICE_NAME, "version": HELPER_VERSION }),
        );
        Ok(status())
    }
}

pub fn uninstall() -> Result<PrivilegedHelperStatus, String> {
    #[cfg(not(windows))]
    {
        Err("Helper privilegiado esta disponivel apenas no Windows.".to_string())
    }

    #[cfg(windows)]
    {
        let script = elevated_script_path("uninstall");
        let script_body = format!(
            r#"
$ErrorActionPreference = 'SilentlyContinue'
sc.exe stop '{service_name}' | Out-Null
Start-Sleep -Milliseconds 800
sc.exe delete '{service_name}' | Out-Null
"#,
            service_name = SERVICE_NAME,
        );
        fs::write(&script, script_body).map_err(|error| error.to_string())?;
        run_elevated_script(&script).inspect_err(|error| {
            audit_helper_event(
                "warn",
                "optimization.helper.uninstall_failed",
                "Remocao do helper privilegiado falhou.",
                json!({ "error": error }),
            );
        })?;
        audit_helper_event(
            "info",
            "optimization.helper.uninstalled",
            "Helper privilegiado removido.",
            json!({ "service_name": SERVICE_NAME }),
        );
        Ok(status())
    }
}

pub fn restart() -> Result<PrivilegedHelperStatus, String> {
    #[cfg(not(windows))]
    {
        Err("Helper privilegiado esta disponivel apenas no Windows.".to_string())
    }

    #[cfg(windows)]
    {
        if !installed_service_source_is_trusted() {
            return Err("Restart bloqueado: o helper instalado aponta para caminho inseguro. Remova o servico como Administrador e reinstale pelo instalador per-machine/Program Files.".to_string());
        }
        let script = elevated_script_path("restart");
        let script_body = format!(
            r#"
$ErrorActionPreference = 'SilentlyContinue'
sc.exe stop '{service_name}' | Out-Null
Start-Sleep -Milliseconds 800
sc.exe start '{service_name}' | Out-Null
"#,
            service_name = SERVICE_NAME,
        );
        fs::write(&script, script_body).map_err(|error| error.to_string())?;
        run_elevated_script(&script).inspect_err(|error| {
            audit_helper_event(
                "warn",
                "optimization.helper.restart_failed",
                "Reinicio do helper privilegiado falhou.",
                json!({ "error": error }),
            );
        })?;
        audit_helper_event(
            "info",
            "optimization.helper.restarted",
            "Helper privilegiado reiniciado.",
            json!({ "service_name": SERVICE_NAME }),
        );
        Ok(status())
    }
}

pub fn start() -> Result<PrivilegedHelperStatus, String> {
    #[cfg(not(windows))]
    {
        Err("Helper privilegiado esta disponivel apenas no Windows.".to_string())
    }

    #[cfg(windows)]
    {
        if !service_installed() {
            return Err("Helper nao esta instalado. Use Reparar helper para reinstalar o servico.".to_string());
        }
        if !installed_service_source_is_trusted() {
            return Err("Start bloqueado: o helper instalado aponta para caminho inseguro. Remova o servico como Administrador e reinstale pelo instalador per-machine/Program Files.".to_string());
        }
        run_service_control("start", "start")?;
        audit_helper_event(
            "info",
            "optimization.helper.started",
            "Helper privilegiado iniciado.",
            json!({ "service_name": SERVICE_NAME }),
        );
        Ok(status())
    }
}

pub fn stop() -> Result<PrivilegedHelperStatus, String> {
    #[cfg(not(windows))]
    {
        Err("Helper privilegiado esta disponivel apenas no Windows.".to_string())
    }

    #[cfg(windows)]
    {
        if !service_installed() {
            return Err("Helper nao esta instalado.".to_string());
        }
        run_service_control("stop", "stop")?;
        audit_helper_event(
            "info",
            "optimization.helper.stopped",
            "Helper privilegiado parado.",
            json!({ "service_name": SERVICE_NAME }),
        );
        Ok(status())
    }
}

/// Proves the HMAC named-pipe channel end to end by sending a signed
/// `HELPER_PING` and verifying the signed pong. Never runs an optimization.
/// Returns actionable diagnostics rather than opaque errors so the UI can guide
/// the user to the right repair step.
pub fn handshake() -> Result<PrivilegedHelperHandshake, String> {
    #[cfg(not(windows))]
    {
        Ok(PrivilegedHelperHandshake {
            ok: false,
            latency_ms: 0,
            helper_version: None,
            message: "Helper privilegiado esta disponivel apenas no Windows.".to_string(),
        })
    }

    #[cfg(windows)]
    {
        if !service_installed() {
            return Ok(PrivilegedHelperHandshake {
                ok: false,
                latency_ms: 0,
                helper_version: helper_version_file(),
                message: "Servico do helper nao esta instalado. Clique em Reparar helper."
                    .to_string(),
            });
        }
        if !service_running() {
            return Ok(PrivilegedHelperHandshake {
                ok: false,
                latency_ms: 0,
                helper_version: helper_version_file(),
                message: "Helper instalado mas parado. Clique em Iniciar helper e teste de novo."
                    .to_string(),
            });
        }

        match handshake_roundtrip() {
            Ok(result) => Ok(result),
            Err(error) => Ok(PrivilegedHelperHandshake {
                ok: false,
                latency_ms: 0,
                helper_version: helper_version_file(),
                message: format!(
                    "Falha no handshake com o helper: {error}. Reinicie o helper e teste de novo."
                ),
            }),
        }
    }
}

#[cfg(windows)]
fn handshake_roundtrip() -> Result<PrivilegedHelperHandshake, String> {
    use std::time::Instant;

    let signing_key = load_helper_signing_key()?;
    let request =
        build_signed_request(HELPER_PING_ACTION, None, safety::CommandSource::ManualUser, true, &signing_key)?;
    let action_id = request.action_id.clone();

    let started = Instant::now();
    let mut pipe = open_helper_pipe()?;
    let request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    write_pipe_frame(&mut pipe, &request_bytes)?;
    let response_bytes = read_pipe_frame(&mut pipe)?;
    let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

    let response: HelperCommandResponse =
        serde_json::from_slice(&response_bytes).map_err(|error| error.to_string())?;
    if response.action_id != action_id || response.request_nonce != request.nonce {
        return Err("resposta com identificadores inesperados".to_string());
    }
    verify_response_signature(&response, &signing_key)?;

    let helper_version = response
        .details
        .get("helperVersion")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .or_else(helper_version_file);

    let message = if response.success {
        format!(
            "Canal seguro do helper respondeu em {latency_ms} ms (versao {}).",
            helper_version.clone().unwrap_or_else(|| "desconhecida".to_string())
        )
    } else {
        response.message.clone()
    };

    Ok(PrivilegedHelperHandshake {
        ok: response.success,
        latency_ms,
        helper_version,
        message,
    })
}

#[cfg(windows)]
fn run_service_control(sc_verb: &str, script_tag: &str) -> Result<(), String> {
    let script = elevated_script_path(script_tag);
    let script_body = format!(
        r#"
$ErrorActionPreference = 'SilentlyContinue'
sc.exe {sc_verb} '{service_name}' | Out-Null
"#,
        sc_verb = sc_verb,
        service_name = SERVICE_NAME,
    );
    fs::write(&script, script_body).map_err(|error| error.to_string())?;
    run_elevated_script(&script)
}

pub fn execute(
    action_name: &str,
    payload: Option<Value>,
    source: safety::CommandSource,
    local_confirmation: bool,
) -> Result<ExecutionResult, String> {
    #[cfg(not(windows))]
    {
        let _ = (action_name, payload, source, local_confirmation);
        return Err("Helper privilegiado esta disponivel apenas no Windows.".to_string());
    }

    #[cfg(windows)]
    {
        if !status().available {
            return Err("Helper privilegiado nao esta instalado e rodando.".to_string());
        }
        if !local_confirmation {
            return Err(
                "Helper privilegiado exige confirmacao local antes de assinar request.".to_string(),
            );
        }
        if !supported_actions()
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(action_name))
        {
            return Err(format!("Acao nao suportada pelo helper: {action_name}"));
        }

        let signing_key = load_helper_signing_key()?;
        let request = build_signed_request(
            action_name,
            payload,
            source,
            local_confirmation,
            &signing_key,
        )?;
        let action_id = request.action_id.clone();

        let mut pipe = open_helper_pipe()?;
        let request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        write_pipe_frame(&mut pipe, &request_bytes)?;
        let response_bytes = read_pipe_frame(&mut pipe)?;
        let response: HelperCommandResponse =
            serde_json::from_slice(&response_bytes).map_err(|error| error.to_string())?;

        if response.action_id != action_id {
            return Err("Helper retornou response com action_id inesperado.".to_string());
        }
        if response.request_nonce != request.nonce {
            return Err("Helper retornou response com nonce inesperado.".to_string());
        }
        verify_response_signature(&response, &signing_key)?;

        let mut message = response.message;
        if !response.success && is_protocol_mismatch_message(&message) {
            audit_helper_event(
                "warn",
                "optimization.helper.protocol_mismatch",
                "Helper privilegiado recusou request por incompatibilidade de versao de protocolo.",
                json!({ "action_name": action_name, "helper_message": message.clone() }),
            );
            message = degraded_helper_message();
        }

        Ok(ExecutionResult {
            success: response.success,
            message,
            details: response.details,
        })
    }
}

fn is_protocol_mismatch_message(message: &str) -> bool {
    message.contains("versao de protocolo")
}

fn degraded_helper_message() -> String {
    "Helper privilegiado desatualizado ou incompativel com esta versao do app. Reinicie o helper para sincronizar as versoes antes de tentar novamente.".to_string()
}

pub fn run_service() {
    #[cfg(windows)]
    {
        if let Err(error) =
            windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        {
            eprintln!("Falha ao iniciar dispatcher do helper: {error}");
        }
    }

    #[cfg(not(windows))]
    {}
}

#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, service_main);

#[cfg(windows)]
fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service_loop() {
        eprintln!("Falha no helper privilegiado: {error}");
    }
}

#[cfg(windows)]
fn run_service_loop() -> windows_service::Result<()> {
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    fs::write(helper_root().join("version.txt"), HELPER_VERSION).ok();
    if let Err(error) = ensure_helper_signing_key_file() {
        eprintln!("Falha ao preparar chave local do helper: {error}");
    }

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let pipe_shutdown = Arc::clone(&shutdown_flag);
    let pipe_thread = thread::spawn(move || run_named_pipe_server(pipe_shutdown));

    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let handler_shutdown = Arc::clone(&shutdown_flag);
    let status_handle =
        service_control_handler::register(SERVICE_NAME, move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                handler_shutdown.store(true, Ordering::SeqCst);
                wake_named_pipe_server();
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    loop {
        if shutdown_rx.try_recv().is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(COMMAND_POLL_MS));
    }

    shutdown_flag.store(true, Ordering::SeqCst);
    wake_named_pipe_server();
    let _ = pipe_thread.join();

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;
    Ok(())
}

#[cfg(windows)]
fn run_named_pipe_server(shutdown: Arc<AtomicBool>) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match create_named_pipe_instance() {
            Ok(handle) => {
                if let Err(error) = accept_named_pipe_client(handle, &shutdown) {
                    eprintln!("Falha no named pipe do helper: {error}");
                    audit_helper_event(
                        "warn",
                        "optimization.helper.pipe_error",
                        "Falha no canal named pipe do helper.",
                        json!({ "error": error }),
                    );
                    thread::sleep(Duration::from_millis(250));
                }
            }
            Err(error) => {
                eprintln!("Falha ao criar named pipe do helper: {error}");
                audit_helper_event(
                    "warn",
                    "optimization.helper.pipe_create_failed",
                    "Helper nao conseguiu criar o named pipe local.",
                    json!({ "error": error }),
                );
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(windows)]
fn accept_named_pipe_client(
    handle: windows::Win32::Foundation::HANDLE,
    shutdown: &AtomicBool,
) -> Result<(), String> {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};

    let connected = match unsafe { ConnectNamedPipe(handle, None) } {
        Ok(()) => true,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => true,
        Err(error) => {
            let _ = unsafe { CloseHandle(handle) };
            return Err(error.to_string());
        }
    };

    if !connected || shutdown.load(Ordering::SeqCst) {
        let _ = unsafe { DisconnectNamedPipe(handle) };
        let _ = unsafe { CloseHandle(handle) };
        return Ok(());
    }

    let raw_handle = handle.0 as usize;
    thread::spawn(move || {
        let handle = windows::Win32::Foundation::HANDLE(raw_handle as _);
        if let Err(error) = handle_named_pipe_connection(handle) {
            eprintln!("Falha ao processar request do helper: {error}");
            audit_helper_event(
                "warn",
                "optimization.helper.pipe_connection_failed",
                "Falha ao processar conexao do named pipe do helper.",
                json!({ "error": error }),
            );
        }
    });
    Ok(())
}

#[cfg(windows)]
fn handle_named_pipe_connection(handle: windows::Win32::Foundation::HANDLE) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Pipes::DisconnectNamedPipe;

    let client = match validate_named_pipe_client(handle) {
        Ok(client) => client,
        Err(error) => {
            audit_helper_event(
                "warn",
                "optimization.helper.client_rejected",
                "Cliente do helper recusado antes de ler request.",
                json!({ "reason": error }),
            );
            let _ = unsafe { DisconnectNamedPipe(handle) };
            let _ = unsafe { CloseHandle(handle) };
            return Err(error);
        }
    };

    let mut pipe = unsafe { fs::File::from_raw_handle(handle.0 as _) };
    let signing_key = load_helper_signing_key()?;
    let response = match read_request_from_pipe(&mut pipe, &signing_key) {
        Ok(request) => {
            audit_helper_event(
                "info",
                "optimization.helper.request_accepted",
                "Request do helper autenticado e autorizado.",
                json!({
                    "action_id": request.action_id.clone(),
                    "action_name": request.action_name.clone(),
                    "source": request.source,
                    "client_pid": client.pid,
                    "client_path": client.path,
                }),
            );
            execute_request(request, &signing_key)
        }
        Err(rejection) => {
            audit_helper_event(
                "warn",
                "optimization.helper.request_rejected",
                "Request do helper recusado.",
                json!({
                    "action_id": rejection.action_id.clone(),
                    "reason": rejection.message.clone(),
                    "client_pid": client.pid,
                    "client_path": client.path,
                }),
            );
            build_error_response(
                &rejection.action_id,
                &rejection.request_nonce,
                rejection.message,
                &signing_key,
            )?
        }
    };

    let response_bytes = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    write_pipe_frame(&mut pipe, &response_bytes)?;
    let raw = pipe.as_raw_handle();
    let _ = unsafe { DisconnectNamedPipe(windows::Win32::Foundation::HANDLE(raw as _)) };
    Ok(())
}

#[cfg(windows)]
fn read_request_from_pipe(
    pipe: &mut fs::File,
    signing_key: &[u8],
) -> Result<HelperCommandRequest, RejectedHelperRequest> {
    let raw = read_pipe_frame(pipe).map_err(|message| RejectedHelperRequest {
        action_id: "unknown".to_string(),
        request_nonce: String::new(),
        message,
    })?;
    let request: HelperCommandRequest =
        serde_json::from_slice(&raw).map_err(|error| RejectedHelperRequest {
            action_id: "unknown".to_string(),
            request_nonce: String::new(),
            message: error.to_string(),
        })?;
    let rejection_action_id = if request.action_id.trim().is_empty() {
        "unknown".to_string()
    } else {
        request.action_id.clone()
    };
    let rejection_nonce = request.nonce.clone();
    validate_request_envelope(&request, signing_key, now_ts()).map_err(|message| {
        RejectedHelperRequest {
            action_id: rejection_action_id.clone(),
            request_nonce: rejection_nonce.clone(),
            message,
        }
    })?;
    validate_request_execution_policy(&request).map_err(|message| RejectedHelperRequest {
        action_id: rejection_action_id.clone(),
        request_nonce: rejection_nonce.clone(),
        message,
    })?;
    mark_request_seen(&request).map_err(|message| RejectedHelperRequest {
        action_id: rejection_action_id,
        request_nonce: rejection_nonce,
        message,
    })?;
    Ok(request)
}

#[cfg(test)]
fn read_request_from_bytes(raw: &[u8], signing_key: &[u8]) -> Result<HelperCommandRequest, String> {
    let request: HelperCommandRequest =
        serde_json::from_slice(raw).map_err(|error| error.to_string())?;
    validate_request_envelope(&request, signing_key, now_ts())?;
    validate_request_execution_policy(&request)?;
    mark_request_seen(&request)?;
    Ok(request)
}

fn execute_request(request: HelperCommandRequest, signing_key: &[u8]) -> HelperCommandResponse {
    let action_id = request.action_id.clone();
    let request_nonce = request.nonce.clone();

    if request.action_name.eq_ignore_ascii_case(HELPER_PING_ACTION) {
        return build_response(
            action_id,
            request_nonce,
            true,
            "pong".to_string(),
            json!({ "pong": true, "helperVersion": HELPER_VERSION }),
            signing_key,
        )
        .unwrap_or_else(|error| HelperCommandResponse {
            action_id: "unknown".to_string(),
            request_nonce: String::new(),
            success: false,
            message: format!("Falha ao assinar pong do helper: {error}"),
            details: json!({ "pong": false }),
            finished_at: now_ts(),
            signature: String::new(),
        });
    }


    let action_name = request.action_name.clone();
    let source = request.source;
    let result = {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        match runtime {
            Ok(runtime) => runtime.block_on(super::execute_privileged_helper_command(
                request.source,
                &request.action_name,
                request.payload,
            )),
            Err(error) => ExecutionResult {
                success: false,
                message: format!("Falha ao criar runtime do helper: {error}"),
                details: json!({ "implemented": true }),
            },
        }
    };

    audit_helper_event(
        if result.success { "info" } else { "warn" },
        "optimization.helper.request_executed",
        "Request do helper executado.",
        json!({
            "action_id": action_id.clone(),
            "action_name": action_name.clone(),
            "source": source,
            "success": result.success,
            "message": result.message.clone(),
            "details": result.details.clone(),
        }),
    );

    build_response(
        action_id,
        request_nonce,
        result.success,
        result.message,
        result.details,
        signing_key,
    )
    .unwrap_or_else(|error| HelperCommandResponse {
        action_id: "unknown".to_string(),
        request_nonce: String::new(),
        success: false,
        message: format!("Falha ao assinar response do helper: {error}"),
        details: json!({ "implemented": true }),
        finished_at: now_ts(),
        signature: String::new(),
    })
}

fn build_signed_request(
    action_name: &str,
    payload: Option<Value>,
    source: safety::CommandSource,
    local_confirmation: bool,
    signing_key: &[u8],
) -> Result<HelperCommandRequest, String> {
    let now = now_ts();
    let payload_sha256 = payload_sha256(payload.as_ref())?;
    let mut request = HelperCommandRequest {
        protocol_version: REQUEST_PROTOCOL_VERSION,
        origin: REQUEST_ORIGIN.to_string(),
        action_id: uuid::Uuid::new_v4().simple().to_string(),
        action_name: action_name.to_string(),
        source,
        local_confirmation,
        payload,
        payload_sha256,
        nonce: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        created_at: now,
        expires_at: now.saturating_add(REQUEST_TTL_SECONDS),
        signature: String::new(),
    };
    request.signature = sign_request(&request, signing_key)?;
    Ok(request)
}

fn validate_request_envelope(
    request: &HelperCommandRequest,
    signing_key: &[u8],
    now: i64,
) -> Result<(), String> {
    if request.protocol_version != REQUEST_PROTOCOL_VERSION {
        return Err("Request recusado: versao de protocolo inesperada.".to_string());
    }
    if request.origin != REQUEST_ORIGIN {
        return Err("Request recusado: origem inesperada.".to_string());
    }
    if !is_action_id(&request.action_id) {
        return Err("Request recusado: action_id invalido.".to_string());
    }
    if !is_nonce(&request.nonce) {
        return Err("Request recusado: nonce invalido.".to_string());
    }
    if request.created_at <= 0
        || request.expires_at <= request.created_at
        || request.expires_at.saturating_sub(request.created_at) > REQUEST_TTL_SECONDS
    {
        return Err("Request recusado: janela temporal invalida.".to_string());
    }
    if request.created_at.saturating_sub(now) > REQUEST_MAX_CLOCK_SKEW_SECONDS {
        return Err("Request recusado: timestamp no futuro.".to_string());
    }
    if now > request.expires_at {
        return Err("Request recusado: expiracao curta vencida.".to_string());
    }
    if payload_sha256(request.payload.as_ref())? != request.payload_sha256 {
        return Err("Request recusado: hash canonico do payload nao confere.".to_string());
    }
    verify_request_signature(request, signing_key)
}

fn validate_request_execution_policy(request: &HelperCommandRequest) -> Result<(), String> {
    if request.action_name.eq_ignore_ascii_case(HELPER_PING_ACTION) {
        // Health-check only: it has no privileged side effects, so it bypasses
        // the optimization allowlist and safety engine. The envelope signature,
        // nonce, TTL and origin were already verified by the caller.
        return Ok(());
    }
    if !supported_actions()
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(&request.action_name))
    {
        return Err(format!(
            "Acao recusada pelo helper: {} nao esta na allowlist.",
            request.action_name
        ));
    }
    if request.source == safety::CommandSource::LocalPolicy {
        return Err(
            "Acao recusada pelo helper: politica local nao pode acionar admin sem usuario."
                .to_string(),
        );
    }
    if !request.local_confirmation {
        return Err("Acao recusada pelo helper: confirmacao local ausente.".to_string());
    }

    validate_payload_paths(&request.action_name, request.payload.as_ref())?;

    let allowed_actions = supported_actions()
        .iter()
        .map(|action| (*action).to_string())
        .collect::<Vec<_>>();
    let safety_context = safety::SafetyContext {
        source: request.source,
        allowed_actions: Some(&allowed_actions),
        local_confirmation: request.local_confirmation,
        privileged_helper_available: true,
    };
    let profile = safety::validate_command(
        &request.action_name,
        request.payload.as_ref(),
        &safety_context,
    )
    .map_err(|error| {
        format!(
            "Acao recusada pelo helper: {} ({})",
            error.reason, error.details
        )
    })?;
    if profile.risk == safety::RiskLevel::Critical {
        return Err("Acao recusada pelo helper: risco critico exige fluxo dedicado.".to_string());
    }

    Ok(())
}

fn mark_request_seen(request: &HelperCommandRequest) -> Result<(), String> {
    let store = SEEN_REQUEST_NONCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut store = store
        .lock()
        .map_err(|_| "Request recusado: memoria de nonces indisponivel.".to_string())?;
    let now = now_ts();
    store.retain(|_, expires_at| *expires_at >= now);

    let nonce_key = format!("nonce:{}", request.nonce);
    let action_key = format!("action:{}", request.action_id);
    if store.contains_key(&nonce_key) || store.contains_key(&action_key) {
        return Err("Request recusado: nonce/action_id reutilizado.".to_string());
    }

    let retention = request
        .expires_at
        .saturating_add(REQUEST_TTL_SECONDS)
        .max(now.saturating_add(REQUEST_TTL_SECONDS));
    store.insert(nonce_key, retention);
    store.insert(action_key, retention);
    Ok(())
}

fn payload_sha256(payload: Option<&Value>) -> Result<String, String> {
    let canonical = match payload {
        Some(value) => canonical_json(value)?,
        None => "null".to_string(),
    };
    Ok(hex::encode(Sha256::digest(canonical.as_bytes())))
}

fn sign_request(request: &HelperCommandRequest, signing_key: &[u8]) -> Result<String, String> {
    let signature_payload = request_signature_payload(request);
    let canonical = canonical_json(&signature_payload)?;
    let mut mac = HmacSha256::new_from_slice(signing_key)
        .map_err(|_| "Chave local de assinatura invalida.".to_string())?;
    mac.update(canonical.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn verify_request_signature(
    request: &HelperCommandRequest,
    signing_key: &[u8],
) -> Result<(), String> {
    if request.signature.len() != 64
        || !request
            .signature
            .chars()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("Request recusado: assinatura local invalida.".to_string());
    }
    let expected = sign_request(request, signing_key)?;
    if !constant_time_eq(expected.as_bytes(), request.signature.as_bytes()) {
        return Err("Request recusado: assinatura local nao confere.".to_string());
    }
    Ok(())
}

fn build_response(
    action_id: String,
    request_nonce: String,
    success: bool,
    message: String,
    details: Value,
    signing_key: &[u8],
) -> Result<HelperCommandResponse, String> {
    let mut response = HelperCommandResponse {
        action_id,
        request_nonce,
        success,
        message,
        details,
        finished_at: now_ts(),
        signature: String::new(),
    };
    response.signature = sign_response(&response, signing_key)?;
    Ok(response)
}

fn build_error_response(
    action_id: &str,
    request_nonce: &str,
    message: String,
    signing_key: &[u8],
) -> Result<HelperCommandResponse, String> {
    build_response(
        action_id.to_string(),
        request_nonce.to_string(),
        false,
        message,
        json!({ "implemented": true }),
        signing_key,
    )
}

fn sign_response(response: &HelperCommandResponse, signing_key: &[u8]) -> Result<String, String> {
    let signature_payload = response_signature_payload(response);
    let canonical = canonical_json(&signature_payload)?;
    let mut mac = HmacSha256::new_from_slice(signing_key)
        .map_err(|_| "Chave local de assinatura invalida.".to_string())?;
    mac.update(canonical.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn verify_response_signature(
    response: &HelperCommandResponse,
    signing_key: &[u8],
) -> Result<(), String> {
    if response.signature.len() != 64
        || !response
            .signature
            .chars()
            .all(|value| value.is_ascii_hexdigit())
    {
        return Err("Response do helper recusada: assinatura invalida.".to_string());
    }
    let expected = sign_response(response, signing_key)?;
    if !constant_time_eq(expected.as_bytes(), response.signature.as_bytes()) {
        return Err("Response do helper recusada: assinatura nao confere.".to_string());
    }
    Ok(())
}

fn request_signature_payload(request: &HelperCommandRequest) -> Value {
    json!({
        "kind": "helper_request_v1",
        "protocolVersion": request.protocol_version,
        "origin": request.origin,
        "actionId": request.action_id,
        "actionName": request.action_name,
        "source": request.source,
        "localConfirmation": request.local_confirmation,
        "payload": request.payload,
        "payloadSha256": request.payload_sha256,
        "nonce": request.nonce,
        "createdAt": request.created_at,
        "expiresAt": request.expires_at,
    })
}

fn response_signature_payload(response: &HelperCommandResponse) -> Value {
    json!({
        "kind": "helper_response_v1",
        "actionId": response.action_id,
        "requestNonce": response.request_nonce,
        "success": response.success,
        "message": response.message,
        "details": response.details,
        "finishedAt": response.finished_at,
    })
}

fn canonical_json(value: &Value) -> Result<String, String> {
    let mut output = String::new();
    write_canonical_value(value, &mut output)?;
    Ok(output)
}

fn write_canonical_value(value: &Value, output: &mut String) -> Result<(), String> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => {
            output.push_str(&serde_json::to_string(value).map_err(|error| error.to_string())?)
        }
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical_value(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).map_err(|error| error.to_string())?);
                output.push(':');
                let value = values
                    .get(key)
                    .ok_or_else(|| "Falha ao canonicalizar payload.".to_string())?;
                write_canonical_value(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

fn validate_payload_paths(action_name: &str, payload: Option<&Value>) -> Result<(), String> {
    let Some(payload) = payload else {
        return Ok(());
    };

    reject_unexpected_path_fields(action_name, payload)
}

fn reject_unexpected_path_fields(action_name: &str, value: &Value) -> Result<(), String> {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let normalized_key = key.to_ascii_lowercase();
                if is_path_like_payload_key(&normalized_key) {
                    return Err(format!(
                        "Request recusado: campo de path nao permitido no helper ({action_name}.{key})."
                    ));
                }
                reject_unexpected_path_fields(action_name, value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_unexpected_path_fields(action_name, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_path_like_payload_key(key: &str) -> bool {
    key.contains("path")
        || key.contains("dir")
        || key.contains("root")
        || key.contains("file")
        || key.contains("exe")
        || key.contains("command")
}

fn is_action_id(value: &str) -> bool {
    value.len() == 32 && value.chars().all(|value| value.is_ascii_hexdigit())
}

fn is_nonce(value: &str) -> bool {
    (32..=128).contains(&value.len())
        && value
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_')
}

#[cfg(all(windows, not(test)))]
fn validate_named_pipe_client(
    handle: windows::Win32::Foundation::HANDLE,
) -> Result<VerifiedPipeClient, String> {
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;

    let mut client_pid = 0_u32;
    unsafe { GetNamedPipeClientProcessId(handle, &mut client_pid) }.map_err(|error| {
        format!("Request recusado: PID real do cliente indisponivel ({error}).")
    })?;
    validate_expected_client_pid(client_pid)
}

#[cfg(any(not(windows), test))]
fn validate_named_pipe_client(
    _handle: windows::Win32::Foundation::HANDLE,
) -> Result<VerifiedPipeClient, String> {
    Ok(VerifiedPipeClient {
        pid: std::process::id(),
        path: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("test")),
    })
}

#[cfg(all(windows, not(test)))]
fn validate_expected_client_pid(client_pid: u32) -> Result<VerifiedPipeClient, String> {
    if client_pid == 0 {
        return Err("Request recusado: PID real do cliente invalido.".to_string());
    }

    let process_path = process_path_by_pid(client_pid).ok_or_else(|| {
        "Request recusado: PID real do cliente nao corresponde a processo ativo.".to_string()
    })?;
    if !exe_path_is_trusted_service_source(&process_path)
        || !exe_signature_is_trusted(&process_path)
    {
        return Err("Request recusado: cliente do pipe nao e origem confiavel.".to_string());
    }

    Ok(VerifiedPipeClient {
        pid: client_pid,
        path: process_path,
    })
}

#[cfg(all(windows, not(test)))]
fn process_path_by_pid(pid: u32) -> Option<PathBuf> {
    let command = format!("$process = Get-Process -Id {pid} -ErrorAction Stop; $process.Path");
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &command])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(windows)]
fn open_helper_pipe() -> Result<fs::File, String> {
    let mut last_error = None;
    for _ in 0..50 {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(HELPER_PIPE_NAME)
        {
            Ok(file) => return Ok(file),
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    Err(format!(
        "Nao foi possivel conectar ao named pipe do helper: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "timeout".to_string())
    ))
}

#[cfg(windows)]
fn wake_named_pipe_server() {
    let _ = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(HELPER_PIPE_NAME);
}

#[cfg(windows)]
fn create_named_pipe_instance() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let pipe_name = wide_null(HELPER_PIPE_NAME);
    let security = PipeSecurity::new()?;
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(pipe_name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            MAX_PIPE_FRAME_BYTES as u32,
            MAX_PIPE_FRAME_BYTES as u32,
            0,
            Some(security.as_ptr()),
        )
    };

    if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
        return Err(windows::core::Error::from_thread().to_string());
    }

    Ok(handle)
}

#[cfg(windows)]
struct PipeSecurity {
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
    attributes: windows::Win32::Security::SECURITY_ATTRIBUTES,
}

#[cfg(windows)]
impl PipeSecurity {
    fn new() -> Result<Self, String> {
        use windows::core::w;
        use windows::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)"),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|error| format!("Falha ao criar ACL do named pipe: {error}"))?;

        if descriptor.is_invalid() {
            return Err("Falha ao criar ACL do named pipe: descritor invalido.".to_string());
        }

        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };

        let security = Self {
            descriptor,
            attributes,
        };
        Ok(security)
    }

    fn as_ptr(&self) -> *const windows::Win32::Security::SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

#[cfg(windows)]
impl Drop for PipeSecurity {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};

        if !self.descriptor.is_invalid() {
            let _ = unsafe { LocalFree(Some(HLOCAL(self.descriptor.0))) };
        }
    }
}

#[cfg(windows)]
fn write_pipe_frame(stream: &mut impl Write, payload: &[u8]) -> Result<(), String> {
    if payload.len() > MAX_PIPE_FRAME_BYTES {
        return Err("Frame do helper excede o limite permitido.".to_string());
    }
    let len = payload.len() as u32;
    stream
        .write_all(&len.to_le_bytes())
        .map_err(|error| error.to_string())?;
    stream
        .write_all(payload)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

#[cfg(windows)]
fn read_pipe_frame(stream: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut len_bytes = [0_u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .map_err(|error| error.to_string())?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len == 0 || len > MAX_PIPE_FRAME_BYTES {
        return Err("Frame do helper possui tamanho invalido.".to_string());
    }
    let mut payload = vec![0_u8; len];
    stream
        .read_exact(&mut payload)
        .map_err(|error| error.to_string())?;
    Ok(payload)
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn helper_signing_key_path() -> PathBuf {
    helper_root().join("request-signing.key")
}

fn helper_signing_key_is_readable() -> bool {
    load_helper_signing_key().is_ok()
}

fn ensure_helper_signing_key_file() -> Result<(), String> {
    let path = helper_signing_key_path();
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, generate_signing_key()).map_err(|error| error.to_string())
}

fn load_helper_signing_key() -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(helper_signing_key_path())
        .map_err(|error| format!("Chave local de assinatura do helper indisponivel: {error}"))?;
    let trimmed = raw.trim();
    let decoded = general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| hex::decode(trimmed))
        .map_err(|_| "Chave local de assinatura do helper possui formato invalido.".to_string())?;
    if decoded.len() < MIN_SIGNING_KEY_LEN {
        return Err("Chave local de assinatura do helper e curta demais.".to_string());
    }
    Ok(decoded)
}

fn generate_signing_key() -> String {
    let mut bytes = Vec::with_capacity(MIN_SIGNING_KEY_LEN);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    general_purpose::STANDARD.encode(bytes)
}

fn supported_actions() -> &'static [&'static str] {
    &[
        "EMPTY_TEMP",
        "PURGE_CLEANUP_QUARANTINE",
        "CLEAR_STANDBY_LIST",
        "STOP_SERVICE",
        "RESTORE_SERVICE",
        "SET_DNS_SERVERS",
        "RESET_WINSOCK_CATALOG",
    ]
}

fn current_exe_is_trusted_service_source() -> bool {
    std::env::current_exe()
        .ok()
        .as_deref()
        .is_some_and(exe_path_is_trusted_service_source)
}

fn current_exe_has_trusted_signature() -> bool {
    std::env::current_exe()
        .ok()
        .as_deref()
        .is_some_and(exe_signature_is_trusted)
}

fn exe_signature_is_trusted(path: &Path) -> bool {
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }

    #[cfg(windows)]
    {
        let command = format!(
            "$sig = Get-AuthenticodeSignature -LiteralPath '{}'; if ($sig.Status -eq 'Valid') {{ exit 0 }} else {{ exit 1 }}",
            ps_escape(&path.display().to_string())
        );
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &command,
            ])
            .no_window()
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

fn installed_service_source_is_trusted() -> bool {
    #[cfg(not(windows))]
    {
        false
    }

    #[cfg(windows)]
    {
        service_binary_path().as_deref().is_some_and(|path| {
            exe_path_is_trusted_service_source(path) && exe_signature_is_trusted(path)
        })
    }
}

#[cfg(windows)]
fn current_user_sid() -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        ])
        .no_window()
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Nao foi possivel descobrir o SID do usuario atual.".to_string());
    }
    let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !sid.starts_with("S-1-") {
        return Err("SID do usuario atual retornou formato inesperado.".to_string());
    }
    Ok(sid)
}

fn exe_path_is_trusted_service_source(path: &Path) -> bool {
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }

    #[cfg(windows)]
    {
        exe_path_is_trusted_service_source_with_roots(path, &trusted_service_roots())
    }
}

#[cfg(windows)]
fn exe_path_is_trusted_service_source_with_roots(path: &Path, roots: &[PathBuf]) -> bool {
    if !path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("analystblaze-desktop.exe"))
    {
        return false;
    }

    path_is_under_any_root(path, roots)
}

#[cfg(windows)]
fn service_binary_path() -> Option<PathBuf> {
    let output = Command::new("sc.exe")
        .args(["qc", SERVICE_NAME])
        .no_window()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().find_map(|line| {
        if !line.contains("BINARY_PATH_NAME") {
            return None;
        }
        line.split_once(':')
            .and_then(|(_, value)| extract_service_exe_path(value.trim()))
    })
}

#[cfg(windows)]
fn extract_service_exe_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(PathBuf::from(&rest[..end]));
    }

    let lower = trimmed.to_ascii_lowercase();
    let end = lower.find(".exe")? + ".exe".len();
    Some(PathBuf::from(trimmed[..end].trim()))
}

#[cfg(windows)]
fn trusted_service_roots() -> Vec<PathBuf> {
    ["PROGRAMFILES", "PROGRAMFILES(X86)", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect()
}

#[cfg(windows)]
fn path_is_under_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    let normalized_path = normalize_for_prefix_check(path);
    roots.iter().any(|root| {
        let mut normalized_root = normalize_for_prefix_check(root);
        if !normalized_root.ends_with('\\') {
            normalized_root.push('\\');
        }
        normalized_path.starts_with(&normalized_root)
    })
}

#[cfg(windows)]
fn normalize_for_prefix_check(path: &Path) -> String {
    let mut value = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    value
}

fn helper_version_file() -> Option<String> {
    fs::read_to_string(helper_root().join("version.txt"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn service_installed() -> bool {
    #[cfg(windows)]
    {
        Command::new("sc.exe")
            .args(["query", SERVICE_NAME])
            .no_window()
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn service_running() -> bool {
    #[cfg(windows)]
    {
        Command::new("sc.exe")
            .args(["query", SERVICE_NAME])
            .no_window()
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("RUNNING"))
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn run_elevated_script(script: &Path) -> Result<(), String> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!(
                "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File \"{}\"'",
                script.display()
            ),
        ])
        .no_window()
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Script elevado retornou status {status}."))
    }
}

#[cfg(windows)]
fn elevated_script_path(action: &str) -> PathBuf {
    snapshot::app_data_dir().join(format!("helper-{action}.ps1"))
}

fn helper_root() -> PathBuf {
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(snapshot::app_data_dir)
        .join("AnalystBlaze")
        .join("Helper")
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn audit_helper_event(
    level: impl Into<String>,
    event: impl Into<String>,
    message: impl Into<String>,
    details: Value,
) {
    let _ = audit::record_event(level, event, message, details);
}

fn ps_escape(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod protocol_tests {
    use serde_json::json;

    use super::*;

    fn signing_key() -> Vec<u8> {
        vec![7; MIN_SIGNING_KEY_LEN]
    }

    fn signed_request(action_name: &str, payload: Option<Value>) -> HelperCommandRequest {
        build_signed_request(
            action_name,
            payload,
            safety::CommandSource::ManualUser,
            true,
            &signing_key(),
        )
        .expect("request should be signed")
    }

    #[test]
    fn signed_request_verifies_canonical_payload() {
        let request = signed_request("EMPTY_TEMP", Some(json!({ "min_age_hours": 24 })));

        validate_request_envelope(&request, &signing_key(), request.created_at)
            .expect("signed envelope should verify");
        validate_request_execution_policy(&request).expect("helper policy should allow request");
    }

    #[test]
    fn signed_request_round_trips_from_canonical_bytes() {
        let request = signed_request("EMPTY_TEMP", Some(json!({ "min_age_hours": 24 })));
        let raw = serde_json::to_vec(&request).expect("request should serialize");

        let decoded =
            read_request_from_bytes(&raw, &signing_key()).expect("signed request should decode");

        assert_eq!(decoded.action_id, request.action_id);
        assert_eq!(decoded.nonce, request.nonce);
    }

    #[test]
    fn signed_response_rejects_tampering() {
        let mut response = build_response(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            true,
            "ok".to_string(),
            json!({ "implemented": true }),
            &signing_key(),
        )
        .expect("response should be signed");

        verify_response_signature(&response, &signing_key())
            .expect("signed response should verify");

        response.success = false;
        let error = verify_response_signature(&response, &signing_key())
            .expect_err("tampered response should fail");

        assert!(error.contains("assinatura"));
    }

    #[test]
    fn tampered_payload_fails_envelope_validation() {
        let mut request = signed_request("EMPTY_TEMP", Some(json!({ "min_age_hours": 24 })));
        request.payload = Some(json!({ "min_age_hours": 1 }));

        let error = validate_request_envelope(&request, &signing_key(), request.created_at)
            .expect_err("tampered payload should fail");

        assert!(error.contains("payload") || error.contains("assinatura"));
    }

    #[test]
    fn expired_request_is_rejected() {
        let mut request = signed_request("EMPTY_TEMP", Some(json!({ "min_age_hours": 24 })));
        request.created_at = request.created_at.saturating_sub(120);
        request.expires_at = request.created_at.saturating_add(REQUEST_TTL_SECONDS);
        request.signature = sign_request(&request, &signing_key()).expect("resign request");

        let error = validate_request_envelope(
            &request,
            &signing_key(),
            request.expires_at.saturating_add(1),
        )
        .expect_err("expired request should fail");

        assert!(error.contains("expiracao"));
    }

    #[test]
    fn reused_nonce_or_action_id_is_rejected() {
        let request = signed_request("EMPTY_TEMP", Some(json!({ "min_age_hours": 24 })));

        mark_request_seen(&request).expect("first request should be accepted");
        let error = mark_request_seen(&request).expect_err("replay should be rejected");

        assert!(error.contains("reutilizado"));
    }

    #[test]
    fn purge_quarantine_rejects_external_root() {
        let external_root = std::env::temp_dir().join("analystblaze-outside-quarantine");
        let request = signed_request(
            "PURGE_CLEANUP_QUARANTINE",
            Some(json!({ "quarantine_root": external_root })),
        );

        let error = validate_request_execution_policy(&request)
            .expect_err("external quarantine root should fail");

        assert!(error.contains("quarantine_root"));
    }

    #[test]
    fn protocol_version_mismatch_is_detected_from_helper_rejection_message() {
        assert!(is_protocol_mismatch_message(
            "Request recusado: versao de protocolo inesperada."
        ));
        assert!(!is_protocol_mismatch_message(
            "Request recusado: nonce/action_id reutilizado."
        ));
    }

    #[test]
    fn degraded_helper_message_points_to_restart() {
        let message = degraded_helper_message();
        assert!(message.contains("Reinicie o helper"));
    }

    #[test]
    fn helper_policy_rejects_action_outside_allowlist() {
        let request = signed_request("APPLY_LATENCY_TWEAKS", None);

        let error = validate_request_execution_policy(&request)
            .expect_err("critical command is not helper-allowlisted");

        assert!(error.contains("allowlist"));
    }

    #[test]
    fn helper_policy_allows_set_dns_servers_with_valid_payload() {
        let request = signed_request(
            "SET_DNS_SERVERS",
            Some(json!({ "adapterName": "Ethernet", "dnsServers": ["1.1.1.1", "8.8.8.8"] })),
        );

        validate_request_execution_policy(&request)
            .expect("set dns servers should now be helper-allowlisted");
    }

    #[test]
    fn helper_policy_allows_winsock_reset_with_explicit_confirmation() {
        let request = signed_request(
            "RESET_WINSOCK_CATALOG",
            Some(json!({ "confirm": "RESET_WINSOCK" })),
        );

        validate_request_execution_policy(&request)
            .expect("winsock reset should now be helper-allowlisted");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn trusted_service_source_accepts_program_files_binary() {
        let roots = vec![PathBuf::from(r"C:\Program Files")];
        let path = Path::new(r"C:\Program Files\AnalystBlaze\analystblaze-desktop.exe");

        assert!(exe_path_is_trusted_service_source_with_roots(path, &roots));
    }

    #[test]
    fn trusted_service_source_rejects_user_writable_debug_binary() {
        let roots = vec![PathBuf::from(r"C:\Program Files")];
        let path = Path::new(
            r"C:\Users\vitor\AppData\Local\AnalystBlaze\cargo-target\debug\analystblaze-desktop.exe",
        );

        assert!(!exe_path_is_trusted_service_source_with_roots(path, &roots));
    }

    #[test]
    fn trusted_service_source_rejects_wrong_binary_name() {
        let roots = vec![PathBuf::from(r"C:\Program Files")];
        let path = Path::new(r"C:\Program Files\AnalystBlaze\other.exe");

        assert!(!exe_path_is_trusted_service_source_with_roots(path, &roots));
    }

    #[test]
    fn extracts_quoted_service_binary_path() {
        let path = extract_service_exe_path(
            r#""C:\Program Files\AnalystBlaze\analystblaze-desktop.exe" --analystblaze-helper-service"#,
        );

        assert_eq!(
            path,
            Some(PathBuf::from(
                r"C:\Program Files\AnalystBlaze\analystblaze-desktop.exe"
            ))
        );
    }

    #[test]
    fn extracts_unquoted_service_binary_path() {
        let path = extract_service_exe_path(
            r"C:\Users\vitor\AppData\Local\AnalystBlaze\cargo-target\debug\analystblaze-desktop.exe --analystblaze-helper-service",
        );

        assert_eq!(
            path,
            Some(PathBuf::from(
                r"C:\Users\vitor\AppData\Local\AnalystBlaze\cargo-target\debug\analystblaze-desktop.exe"
            ))
        );
    }
}
