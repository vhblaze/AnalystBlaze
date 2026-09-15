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

; Holds the pre-correction $INSTDIR only while NSIS_HOOK_PREINSTALL is
; relocating an install found outside Program Files - see the comment
; there. Declared at file scope because NSIS `Var` is not legal inside a
; macro body.
Var PerMachineInstDirBeforeFix

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'sc.exe stop AnalystBlazeHelper'
  Pop $0
  ; sc.exe stop only signals the SERVICE process (analystblaze-desktop.exe
  ; --analystblaze-helper-service) - it does NOT terminate
  ; analystblaze-frame-engine.exe (vendored PresentMon, renamed - see
  ; installer/THIRD-PARTY-NOTICES.txt - to avoid a bare third-party binary
  ; name sitting in the install folder), which that service spawns as its
  ; own child process for ground-truth FPS capture during Game Mode (see
  ; frame_capture_control.rs). A capture still in flight (or one that never
  ; self-terminated) is left running, orphaned, holding the file locked -
  ; a real user (Nicholas Maack, 2026-09) hit exactly this: "Error opening
  ; file for writing: ...\PresentMon.exe" (before the rename).
  nsExec::ExecToLog 'taskkill /F /IM analystblaze-frame-engine.exe'
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
    ; sign a cryptic message buried in Settings. This also self-heals an
    ; already-broken install: the in-app auto-updater (updater.rs) runs
    ; this exact installer passively, so a user in Nely's situation gets
    ; fixed on their very next automatic update, with no action needed.
    ;
    ; StrCmp (the bare instruction, not LogicLib's ==) is case-insensitive,
    ; matching how Windows itself treats paths - deliberate here since
    ; $INSTDIR came from RestorePreviousInstallLocation (a registry value)
    ; or free-form wizard text, neither guaranteed to match the casing
    ; $PROGRAMFILES64/$PROGRAMFILES expand to.
    StrLen $R7 "$PROGRAMFILES64"
    StrCpy $R8 "$INSTDIR" $R7
    StrLen $R9 "$PROGRAMFILES"
    StrCpy $R6 "$INSTDIR" $R9
    StrCmp $R8 "$PROGRAMFILES64" installdir_is_safe 0
    StrCmp $R6 "$PROGRAMFILES" installdir_is_safe installdir_needs_fix
    installdir_needs_fix:
      StrCpy $PerMachineInstDirBeforeFix "$INSTDIR"
      ${If} ${RunningX64}
        StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
      DetailPrint "AnalystBlaze: instalacao anterior fora de Program Files ($PerMachineInstDirBeforeFix) - corrigindo para $INSTDIR"
      SetOutPath $INSTDIR

      ; Files land in the new place, but a shortcut the user already has
      ; still points at $PerMachineInstDirBeforeFix until retargeted - the
      ; base template's own CreateOrUpdate*Shortcut functions don't handle
      ; this (they only migrate a same-folder binary rename, and skip
      ; entirely on a plain update). Only touch a shortcut that actually
      ; exists; never create one that wasn't there.
      ${If} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
        !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      ${EndIf}
      ${If} ${FileExists} "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
        !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      ${EndIf}
    installdir_is_safe:
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
