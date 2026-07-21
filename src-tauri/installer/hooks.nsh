; Custom NSIS hooks for the AnalystBlazeHelper privileged Windows service.
;
; The service (see src-tauri/src/optimizations/privileged_helper.rs) points at
; this exact same app executable, just launched with the
; `--analystblaze-helper-service` flag. Registration/removal of the service is
; delegated to the single canonical script installer/helper-service.ps1, which
; is bundled as a Tauri resource (see tauri.conf.json -> bundle.resources) and
; therefore lands next to the app under $INSTDIR at install time.
;
; A per-machine NSIS install already runs elevated, so creating the service and
; its %ProgramData% root here raises no extra UAC prompt. On a plain update the
; PREINSTALL hook stops the running service (releasing the file lock so the new
; binary overwrites cleanly) and POSTINSTALL recreates + starts it, so the app,
; its version.txt and the IPC protocol_version stay in sync.
;
; Every command here is best-effort: a fresh install (no service yet) or an
; already-stopped service simply results in a non-zero exit code, which is
; ignored. Tauri v2 always places bundle.resources under $INSTDIR\resources
; (confirmed against the Tauri docs), so there's a single correct path here -
; no more guessing between $INSTDIR and $INSTDIR\resources.

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\helper-service.ps1" -Action install -ExePath "$INSTDIR\analystblaze-desktop.exe"'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\helper-service.ps1" -Action uninstall'
  Pop $0
  ; Safety net: ensure the service is gone even if the script above didn't run.
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
  nsExec::ExecToLog 'sc.exe delete AnalystBlazeHelper'
  Pop $0
!macroend
