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
; Every command here is best-effort: a fresh install (no service yet), an
; already-stopped service, or a missing candidate script path simply results in
; a non-zero exit code, which is ignored. Two candidate paths are tried because
; the bundler may place resources at $INSTDIR or under $INSTDIR\resources.

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\helper-service.ps1" -Action install -ExePath "$INSTDIR\analystblaze-desktop.exe"'
  Pop $0
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\helper-service.ps1" -Action install -ExePath "$INSTDIR\analystblaze-desktop.exe"'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\helper-service.ps1" -Action uninstall'
  Pop $0
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\helper-service.ps1" -Action uninstall'
  Pop $0
  ; Safety net: ensure the service is gone even if neither script path resolved.
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
  nsExec::ExecToLog 'sc.exe delete AnalystBlazeHelper'
  Pop $0
!macroend
