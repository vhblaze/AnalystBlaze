# Windows Latency Optimizer - Wave 1

## Implemented

- Foreground Burst Mode for a detected latency-sensitive foreground/game process.
- Background Quiet Mode for eligible third-party background processes.
- Uplink Pressure Relief Stage 1 using only user-mode reversible process controls.
- Agent self-throttle during active latency-sensitive sessions:
  - realtime backend publish is coalesced to a lower frequency;
  - realtime status polling is reduced;
  - local UI samples continue to be published locally.
- Snapshot-backed rollback for process priority and process efficiency changes.
- User undo via the `restore_latency_session` Tauri command.
- App-exit rollback through the existing tray quit flow.

## Windows Controls Used

Only documented process APIs are used in this wave:

- `OpenProcess` with limited query/set-information rights.
- `SetPriorityClass`.
- `GetProcessInformation` / `SetProcessInformation` for memory priority and process power throttling.

The implementation never requests `PROCESS_ALL_ACCESS`, never enables `SeDebugPrivilege`, and never uses `REALTIME_PRIORITY_CLASS`.

## Safety Model

- Foreground Burst and Uplink Stage 1 are sensitive actions.
- Remote execution requires signed server policy plus local confirmation.
- Every applied change creates a local snapshot before the action is reported as reversible.
- Protected processes remain excluded through the existing safety layer.
- Snapshot restore is attempted on user undo, app exit, timeout/session expiry, or explicit restore command.

## Plan Gating

- Starter: manual diagnostics and existing manual Gamer Mode only.
- Pro/Family: low-risk unattended optimizer actions can be policy-enabled by the backend.
- The desktop treats backend policy as the source of truth and still applies local safety gates.

## Privacy Safeguards

- The agent should avoid sending raw process paths, SSID, BSSID, or raw remote peer data.
- Latency summaries are intended to contain aggregate timing, confidence, rollback, and reason-code data only.
- Full path and Wi-Fi identifiers are local diagnostics unless a future explicit diagnostic opt-in is added.

## Intentionally Not Automated In Wave 1

- Wi-Fi interface mutation is telemetry/advisory only until a dedicated documented WLAN wrapper and restore harness are added.
- CPU Sets / hybrid CPU isolation are advisory only until topology validation and negative-effect rollback are proven.
- Power-plan switching is not changed by this wave; future work should use documented Power APIs with AC-only checks and exact restore snapshots.
- Per-application QoS throttling is not automated without an admin/helper path, signed artifacts, TTL rollback, and explicit opt-in.
- No process killing, thread suspension, UAC bypass, driver work, or global TCP/NIC registry tuning is implemented.

## Validation

Run from `AnalystBlaze-desktop/src-tauri`:

```powershell
cargo test
```

Expected result for this wave:

- Rust unit tests pass.
- Desktop security harness passes.
- Safety tests confirm Foreground Burst and Uplink Stage 1 require signed policy plus local confirmation and snapshots.

## Lab-Only Follow-Up

Use `docs/windows-lab-validation.md` for disposable Windows VM checks around service ACLs, helper binary signature validation, DLL search-order simulation, and process/module telemetry. Those checks are not part of normal PR CI.
