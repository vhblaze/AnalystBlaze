; Custom NSIS hooks for the AnalystBlazeHelper privileged Windows service.
;
; The service (see src-tauri/src/optimizations/privileged_helper.rs) points at
; this exact same app executable, just launched with the
; `--analystblaze-helper-service` flag. On a per-machine update, the installer
; has to overwrite that executable while the service may still be holding it
; open, and the already-running service process keeps executing the OLD code
; in memory even after the file on disk is replaced - so app and helper can
; end up on mismatched versions until the service is restarted. These hooks
; stop the service before install (releasing the file lock and ensuring a
; clean overwrite) and start it again after (so the new binary and its
; version.txt / IPC protocol_version take effect immediately). Every command
; here is best-effort: a fresh install (no service yet) or an already-stopped
; service simply results in these commands returning a non-zero exit code,
; which is ignored.

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'sc.exe start AnalystBlazeHelper'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
  nsExec::ExecToLog 'sc.exe delete AnalystBlazeHelper'
  Pop $0
!macroend
