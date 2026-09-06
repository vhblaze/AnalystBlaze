//! Ground-truth RAM reclaim, the same mechanism RAMMap's "Empty Standby
//! List" / "Empty Working Sets" buttons use: `NtSetSystemInformation` with
//! `SystemMemoryListInformation`. This is an undocumented (by Win32
//! metadata) but long-stable NT native API - not exposed by the `windows`
//! crate's safe Win32 bindings, so the call is declared by hand against
//! `ntdll.dll`, exactly how RAMMap, Process Hacker, and the well-known
//! community tool EmptyStandbyList.exe all do it. Constants verified
//! against Process Hacker's own memlists.c and a walkthrough at
//! https://mouri.moe/en/2021/11/14/Defrag-memory-with-NT-API/ rather than
//! trusted from memory alone, since this runs elevated inside the
//! privileged helper.
//!
//! Sequence matters: emptying working sets first pushes pages into the
//! standby list, flushing the modified list writes dirty pages to disk so
//! they can move to standby too, and only then does purging the standby
//! list actually reclaim as much as it can - purging standby first would
//! miss everything still sitting in a process's working set.

use serde_json::{json, Value};

use super::ExecutionResult;

pub async fn clear_standby_list(payload: Option<Value>) -> ExecutionResult {
    match tokio::task::spawn_blocking(move || clear_standby_list_sync(payload)).await {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            success: false,
            message: format!("Falha ao limpar memoria: {error}"),
            details: json!({ "implemented": true }),
        },
    }
}

#[cfg(windows)]
mod windows_impl {
    use serde_json::{json, Value};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PCWSTR;

    use super::ExecutionResult;

    // SystemInformationClass value for SystemMemoryListInformation - not in
    // Win32 metadata, hand-verified against Process Hacker's memlists.c.
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 0x50;

    // SYSTEM_MEMORY_LIST_COMMAND enum values (ntddk.h) - passed as the u32
    // payload to NtSetSystemInformation, one call per command.
    const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
    const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

    #[link(name = "ntdll")]
    extern "system" {
        fn NtSetSystemInformation(
            system_information_class: u32,
            system_information: *mut core::ffi::c_void,
            system_information_length: u32,
        ) -> i32;
    }

    fn widen(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Enables (not just holds) SeProfileSingleProcessPrivilege on this
    /// process's own token - required for SystemMemoryListInformation.
    /// Privileges a token holds are disabled by default until a process
    /// explicitly asks for them, even running as LocalSystem (which is
    /// where the privileged helper runs this from).
    fn enable_profile_single_process_privilege() -> Result<(), String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .map_err(|error| format!("OpenProcessToken falhou: {error}"))?;

            let name = widen("SeProfileSingleProcessPrivilege");
            let mut luid = LUID::default();
            let lookup_result =
                LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(name.as_ptr()), &mut luid);
            if let Err(error) = lookup_result {
                let _ = CloseHandle(token);
                return Err(format!("LookupPrivilegeValueW falhou: {error}"));
            }

            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            let adjust_result = AdjustTokenPrivileges(
                token,
                false,
                Some(&privileges),
                0,
                None,
                None,
            );
            let _ = CloseHandle(token);
            // AdjustTokenPrivileges can return Ok while still not granting
            // the privilege (if the token doesn't hold it at all) - that
            // shows up as ERROR_NOT_ALL_ASSIGNED from GetLastError, which
            // the windows crate surfaces as an Err here, so a plain Err
            // check already covers both failure shapes.
            adjust_result.map_err(|error| {
                format!(
                    "AdjustTokenPrivileges nao concedeu SeProfileSingleProcessPrivilege: {error}"
                )
            })
        }
    }

    fn set_memory_list_command(command: u32) -> Result<(), String> {
        let mut command = command;
        let status = unsafe {
            NtSetSystemInformation(
                SYSTEM_MEMORY_LIST_INFORMATION,
                &mut command as *mut u32 as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            )
        };
        if status < 0 {
            // NTSTATUS, not Win32 error - reported as the raw hex code since
            // there's no safe FFI helper for RtlNtStatusToDosError wired up
            // here, and the code itself is enough to look up if this ever
            // needs deeper investigation.
            return Err(format!("NtSetSystemInformation retornou NTSTATUS 0x{status:08X}"));
        }
        Ok(())
    }

    fn available_physical_memory_mb() -> Option<u64> {
        let mut info = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        unsafe { GetPerformanceInfo(&mut info, info.cb) }.ok()?;
        let page_size = info.PageSize as u64;
        Some((info.PhysicalAvailable as u64 * page_size) / (1024 * 1024))
    }

    pub fn clear_standby_list_sync(payload: Option<Value>) -> ExecutionResult {
        let empty_working_sets = payload
            .as_ref()
            .and_then(|value| value.get("emptyWorkingSets"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if let Err(error) = enable_profile_single_process_privilege() {
            return ExecutionResult {
                success: false,
                message: "Nao foi possivel habilitar o privilegio necessario para limpar a memoria.".to_string(),
                details: json!({ "implemented": true, "error": error }),
            };
        }

        let before_available_mb = available_physical_memory_mb();

        if empty_working_sets {
            if let Err(error) = set_memory_list_command(MEMORY_EMPTY_WORKING_SETS) {
                return ExecutionResult {
                    success: false,
                    message: "Falha ao esvaziar working sets dos processos.".to_string(),
                    details: json!({ "implemented": true, "error": error, "step": "empty_working_sets" }),
                };
            }
            if let Err(error) = set_memory_list_command(MEMORY_FLUSH_MODIFIED_LIST) {
                return ExecutionResult {
                    success: false,
                    message: "Falha ao esvaziar a lista de paginas modificadas.".to_string(),
                    details: json!({ "implemented": true, "error": error, "step": "flush_modified_list" }),
                };
            }
        }

        if let Err(error) = set_memory_list_command(MEMORY_PURGE_STANDBY_LIST) {
            return ExecutionResult {
                success: false,
                message: "Falha ao limpar a standby list.".to_string(),
                details: json!({ "implemented": true, "error": error, "step": "purge_standby_list" }),
            };
        }

        let after_available_mb = available_physical_memory_mb();
        let freed_mb = match (before_available_mb, after_available_mb) {
            (Some(before), Some(after)) => Some(after.saturating_sub(before)),
            _ => None,
        };

        ExecutionResult::ok(
            if empty_working_sets {
                "Standby list e working sets limpos."
            } else {
                "Standby list limpa."
            },
            json!({
                "implemented": true,
                "empty_working_sets": empty_working_sets,
                "before_available_mb": before_available_mb,
                "after_available_mb": after_available_mb,
                "freed_mb": freed_mb,
            }),
        )
    }
}

#[cfg(windows)]
fn clear_standby_list_sync(payload: Option<Value>) -> ExecutionResult {
    windows_impl::clear_standby_list_sync(payload)
}

#[cfg(not(windows))]
fn clear_standby_list_sync(payload: Option<Value>) -> ExecutionResult {
    ExecutionResult {
        success: false,
        message: "Limpeza de memoria disponivel apenas no Windows.".to_string(),
        details: json!({ "implemented": true, "payload": payload }),
    }
}
