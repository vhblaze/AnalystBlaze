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
  ; sc.exe stop only signals the SERVICE process (analystblaze-desktop.exe
  ; --analystblaze-helper-service) - it does NOT terminate PresentMon.exe,
  ; which that service spawns as its own child process for ground-truth FPS
  ; capture during Game Mode (see frame_capture_control.rs). A capture still
  ; in flight (or one that never self-terminated) is left running, orphaned,
  ; holding PresentMon.exe locked - a real user (Nicholas Maack, 2026-09) hit
  ; exactly this: "Error opening file for writing: ...\PresentMon.exe".
  nsExec::ExecToLog 'taskkill /F /IM PresentMon.exe'
  Pop $0
  ; Same reasoning for the main app's own executable, in case the user has
  ; it open (not just the helper service) while installing an update.
  nsExec::ExecToLog 'taskkill /F /IM analystblaze-desktop.exe'
  Pop $0

  !if "${INSTALLMODE}" == "perMachine"
    ; The privileged helper (privileged_helper.rs::exe_path_is_trusted_service_source)
    ; refuses to register its Windows service unless AnalystBlaze runs from
    ; Program Files/Program Files (x86) - a security boundary against any
    ; arbitrary exe registering itself as a SYSTEM service, which must not be
    ; relaxed. The base template's Choose Directory page still lets a user
    ; type or browse to anywhere, and a real one did (Nely, 2026-09-14),
    ; leaving the helper permanently and silently unavailable - the only
    ; sign a cryptic message buried in Settings. There is no legitimate
    ; reason for THIS app's per-machine install to live anywhere else, so
    ; the location is pinned here to the exact same default the template's
    ; own .onInit computes (see MULTIUSER_USE_PROGRAMFILES64 above),
    ; regardless of what the Directory page showed.
    ;
    ; Skipped during a silent/passive run ($PassiveMode = 1, e.g. the
    ; in-app auto-updater - see SkipIfPassive) on purpose: that path never
    ; shows the Directory page at all and reuses whatever location is
    ; already registered (RestorePreviousInstallLocation), so pinning here
    ; too would silently relocate an already-broken existing install mid
    ; background-update, leaving old files orphaned at the previous path
    ; instead of actually fixing anything. A user in that situation needs to
    ; run the downloaded installer by hand (non-passive) at least once,
    ; which this DOES correct, before auto-update can take over safely.
    ${IfNot} $PassiveMode = 1
      ${If} ${RunningX64}
        StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
      SetOutPath $INSTDIR
    ${EndIf}
  !endif
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
