//! Cheap "is this process in active use right now" checks, so an automatic
//! cleanup step (closing background apps, pausing services) never touches
//! something the user is actually watching, listening to, or talking
//! through. Deliberately narrow: this exists to answer one question once,
//! at the moment Modo Gamer decides what to close - never a polling loop
//! during the session, which would just reintroduce the kind of background
//! overhead optimizations::focus::should_pause_heavy_scans() already exists
//! to avoid while a game is running.
//!
//! Two signals, both cheap:
//! - The foreground window's owning process - a single GetForegroundWindow
//!   call, effectively free.
//! - Whether the process has an active Windows Core Audio session, checked
//!   on BOTH the render (speaker/headset output - catches Spotify, a
//!   YouTube tab playing on a second monitor) and capture (microphone
//!   input - catches Voicemod actively processing a mic feed, which
//!   produces no output sound of its own) device flows. A single moderate
//!   COM query per flow, not a loop.
//!
//! What this deliberately does NOT attempt: telling a browser's background
//! tabs apart from its visible one (would need per-browser integration,
//! not a generic Windows API) or detecting "visible but silent on a second
//! monitor" (real occlusion detection is expensive). Browsers are handled
//! by never being on the closable list at all - see windows_actions.rs's
//! GAME_MODE_CLOSABLE_APPS - rather than trying to solve tab-level
//! granularity here.

use std::collections::HashSet;

#[cfg(windows)]
pub fn foreground_process_id() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

#[cfg(not(windows))]
pub fn foreground_process_id() -> Option<u32> {
    None
}

/// Process ids with at least one currently-active audio session, across
/// both the default playback device (things making sound) and the default
/// recording device (things actively reading the microphone, like
/// Voicemod). "Active" here is Core Audio's own AudioSessionStateActive -
/// the session has a live stream right now, not just an app that
/// theoretically could play sound.
///
/// Best-effort: any COM failure along the way (no default device, driver
/// oddity, etc.) is swallowed and that flow just contributes no pids -
/// callers should already treat "not in this set" as "not confirmed
/// active", not "confirmed idle", so a failure here only ever costs
/// caution, never causes closing something that's actually in use.
#[cfg(windows)]
pub fn processes_with_active_audio_session() -> HashSet<u32> {
    use windows::Win32::Media::Audio::{
        eCapture, eMultimedia, eRender, AudioSessionStateActive, EDataFlow, IMMDeviceEnumerator,
        MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    let mut active = HashSet::new();

    unsafe {
        // RPC_E_CHANGED_MODE (COM already initialized on this thread with a
        // different concurrency model) is fine - it means COM is already
        // usable here, not that this call failed to make it so. Anything
        // else genuinely blocks the calls below, so bail with an empty set.
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let already_initialized_differently = init.0 == windows::Win32::Foundation::RPC_E_CHANGED_MODE.0;
        if init.is_err() && !already_initialized_differently {
            return active;
        }

        let Ok(enumerator) =
            CoCreateInstance::<Option<&windows::core::IUnknown>, IMMDeviceEnumerator>(
                &MMDeviceEnumerator,
                None,
                CLSCTX_ALL,
            )
        else {
            if init.is_ok() {
                CoUninitialize();
            }
            return active;
        };

        for flow in [eRender, eCapture] as [EDataFlow; 2] {
            collect_active_sessions_for_flow(&enumerator, flow, eMultimedia, &mut active);
        }

        if init.is_ok() {
            CoUninitialize();
        }
    }

    #[cfg(windows)]
    unsafe fn collect_active_sessions_for_flow(
        enumerator: &windows::Win32::Media::Audio::IMMDeviceEnumerator,
        flow: windows::Win32::Media::Audio::EDataFlow,
        role: windows::Win32::Media::Audio::ERole,
        active: &mut HashSet<u32>,
    ) {
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{IAudioSessionControl2, IAudioSessionManager2};
        use windows::Win32::System::Com::CLSCTX_ALL;

        let Ok(device) = enumerator.GetDefaultAudioEndpoint(flow, role) else {
            return;
        };
        let Ok(session_manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
        else {
            return;
        };
        let Ok(sessions) = session_manager.GetSessionEnumerator() else {
            return;
        };
        let Ok(count) = sessions.GetCount() else {
            return;
        };
        for index in 0..count {
            let Ok(session) = sessions.GetSession(index) else {
                continue;
            };
            let Ok(session2): windows::core::Result<IAudioSessionControl2> = session.cast()
            else {
                continue;
            };
            let Ok(state) = session2.GetState() else {
                continue;
            };
            if state == AudioSessionStateActive {
                if let Ok(pid) = session2.GetProcessId() {
                    active.insert(pid);
                }
            }
        }
    }

    active
}

#[cfg(not(windows))]
pub fn processes_with_active_audio_session() -> HashSet<u32> {
    HashSet::new()
}

/// True if `pid` looks like it's genuinely in use right now - either it
/// owns the foreground window, or it has a live audio session (playing
/// sound or reading the microphone). Everything not in active use should
/// still only be closed if it's also on a deliberately curated allowlist
/// (see windows_actions.rs) - this function alone is not a safety
/// mechanism, just the "are they actually looking at or listening to this"
/// signal that allowlist membership gets combined with.
pub fn is_in_active_use(pid: u32, foreground_pid: Option<u32>, active_audio_pids: &HashSet<u32>) -> bool {
    foreground_pid == Some(pid) || active_audio_pids.contains(&pid)
}

#[cfg(test)]
mod manual_diagnostics {
    // Not a real assertion-based test - `cargo test -- --ignored --nocapture
    // manual_diagnostics::print_live_state` prints what this machine's own
    // foreground window and active audio sessions actually resolve to right
    // now, so the COM path (never used elsewhere in this codebase before)
    // gets checked against real, live system state instead of trusting a
    // clean compile alone.
    #[test]
    #[ignore]
    fn print_live_state() {
        let foreground = super::foreground_process_id();
        println!("foreground_process_id() = {foreground:?}");
        let active = super::processes_with_active_audio_session();
        println!("processes_with_active_audio_session() = {active:?}");

        let mut system = sysinfo::System::new_all();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        for pid in active.iter().copied() {
            let name = system
                .process(sysinfo::Pid::from_u32(pid))
                .map(|process| process.name().to_string_lossy().to_string());
            println!("  active audio pid {pid} -> {name:?}");
        }
        if let Some(pid) = foreground {
            let name = system
                .process(sysinfo::Pid::from_u32(pid))
                .map(|process| process.name().to_string_lossy().to_string());
            println!("  foreground pid {pid} -> {name:?}");
        }
    }
}
