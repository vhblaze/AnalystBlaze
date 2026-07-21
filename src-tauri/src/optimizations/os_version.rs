use serde::Serialize;
use std::sync::OnceLock;

/// Windows 11's first public build. Microsoft shipped it as a version bump
/// of the same NT 10.0 kernel line rather than a new major version, so the
/// only reliable way to tell it apart from Windows 10 is the build number -
/// `ProductName`/`sysinfo`'s OS label can still read "Windows 10" on some
/// builds and isn't a safe signal.
const WINDOWS_11_MIN_BUILD: u32 = 22000;
/// Windows 10's first public build (RTM 1507).
const WINDOWS_10_MIN_BUILD: u32 = 10240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowsGeneration {
    Windows10,
    Windows11,
    /// Anything we can't confidently place (older Windows, a read failure,
    /// or non-Windows in dev/CI). Optimizations that only make sense on a
    /// specific generation should treat this the same as "not applicable"
    /// rather than guessing.
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WindowsVersionInfo {
    pub generation: WindowsGeneration,
    pub build_number: Option<u32>,
}

static DETECTED: OnceLock<WindowsVersionInfo> = OnceLock::new();

/// Detected once per process and cached - the OS doesn't change under a
/// running app, and this reads the registry so callers on a hot path
/// (telemetry ticks, optimization actions) shouldn't pay for it repeatedly.
pub fn detected() -> WindowsVersionInfo {
    *DETECTED.get_or_init(detect_windows_version)
}

pub fn generation_from_build(build_number: Option<u32>) -> WindowsGeneration {
    match build_number {
        Some(build) if build >= WINDOWS_11_MIN_BUILD => WindowsGeneration::Windows11,
        Some(build) if build >= WINDOWS_10_MIN_BUILD => WindowsGeneration::Windows10,
        _ => WindowsGeneration::Unknown,
    }
}

#[cfg(windows)]
fn detect_windows_version() -> WindowsVersionInfo {
    let build_number = read_current_build_number();
    WindowsVersionInfo {
        generation: generation_from_build(build_number),
        build_number,
    }
}

#[cfg(windows)]
fn read_current_build_number() -> Option<u32> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let raw: String = key.get_value("CurrentBuildNumber").ok()?;
    raw.trim().parse::<u32>().ok()
}

#[cfg(not(windows))]
fn detect_windows_version() -> WindowsVersionInfo {
    WindowsVersionInfo {
        generation: WindowsGeneration::Unknown,
        build_number: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_22000_and_above_is_windows_11() {
        assert_eq!(generation_from_build(Some(22000)), WindowsGeneration::Windows11);
        assert_eq!(generation_from_build(Some(26100)), WindowsGeneration::Windows11);
    }

    #[test]
    fn build_between_10240_and_21999_is_windows_10() {
        assert_eq!(generation_from_build(Some(10240)), WindowsGeneration::Windows10);
        assert_eq!(generation_from_build(Some(19045)), WindowsGeneration::Windows10);
        assert_eq!(generation_from_build(Some(21999)), WindowsGeneration::Windows10);
    }

    #[test]
    fn build_below_10240_or_missing_is_unknown() {
        assert_eq!(generation_from_build(Some(9600)), WindowsGeneration::Unknown);
        assert_eq!(generation_from_build(None), WindowsGeneration::Unknown);
    }
}
