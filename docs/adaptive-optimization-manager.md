# Adaptive Optimization Service Manager

## Scope

This manager coordinates existing telemetry, energy, latency, process, startup, and snapshot modules. It is intentionally conservative: user-mode reversible controls are applied first, while NIC/DNS/Winsock changes are emitted as an admin/helper plan until adapter-specific rollback is implemented.

## Implemented Controls

- Foreground Burst orchestration for latency-sensitive sessions.
- Background Quiet Mode for eligible third-party background processes.
- Game process priority uplift using documented process APIs.
- Game process affinity normalization with snapshot rollback.
- Process memory priority and EcoQoS/background power throttling for eligible background apps.
- Idle-gated eco-mode decision using `GetLastInputInfo`-backed idle seconds.
- Before/after observations containing CPU, RAM, active process count, ping, jitter, packet loss, power profile, and app-impact score.
- Backend-ready `historyReport.latencySummary` output for the existing performance history route.
- HMAC coverage by keeping ping/jitter/app-impact fields inside existing signed telemetry/history payload shapes.

## Admin-Gated Network Plan

The manager prepares, but does not silently execute, plans for:

- DNS flush: `ipconfig /flushdns`
- Winsock reset: `netsh winsock reset`
- DNS server changes
- NIC advanced properties such as Interrupt Moderation, Flow Control, Energy Efficient Ethernet, and Jumbo Packet

Those actions require explicit user consent, elevation/admin helper, and rollback snapshots where possible. Winsock reset is marked manual-only because it is disruptive and usually requires reboot.

## Not Automated Yet

- Vendor-specific NIC property mutation.
- Screen brightness changes.
- Winsock reset execution.
- DNS server replacement without DNS snapshot/restore support.
- Guaranteed "30%" improvement claims. The agent measures before/after and reports confidence rather than promising gains for upstream ISP or server-side bottlenecks.

## Commands

- `APPLY_ADAPTIVE_OPTIMIZATION`

Remote execution requires signed server policy plus local confirmation. Starter remains manual-only; Pro/Family can receive low-risk policy-enabled automation.

## Manual Validation Prompts

Use a disposable Windows 10/11 VM or local test machine:

1. Simulate game running:
   - Open a known game or configure a harmless test process name in the existing detection payload.
   - Run `APPLY_ADAPTIVE_OPTIMIZATION` with `includeNetworkAdminTweaks=false`.
   - Confirm priority/background steps create snapshots and restore cleanly.
2. Simulate idle:
   - Set `idleEcoThresholdSeconds` low in a lab payload.
   - Confirm eco step only runs when idle threshold is met and can be restored by snapshots.
3. Simulate high ping:
   - Temporarily saturate local upload with a benign lab tool.
   - Confirm ping/jitter fields appear in before/after observations and the manager avoids promising external RTT reduction.
4. Simulate network admin request:
   - Run with `includeNetworkAdminTweaks=true` and without `networkChangesConfirmed`.
   - Confirm status is `blocked_user_consent_required`.
   - Run with confirmation but without elevation.
   - Confirm status is `blocked_elevation_required`.

## CI

Desktop CI runs:

```powershell
cargo test optimizations::
cargo test --test security_harness
```
