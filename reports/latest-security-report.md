# AnalystBlaze Desktop Security Pentest Report

| ID | Severity | Status | Title | Remediation |
|---|---|---|---|---|
| ABZ-DESK-DEEPLINK-001 | high | pass | Deep-link scheme is scoped to analystblaze auth callbacks | Keep custom protocol handling limited to the expected analystblaze scheme and signed auth callback flow. |
| ABZ-DESK-CAP-001 | critical | pass | Tauri capabilities avoid filesystem, shell, and allow-all grants | Grant only command scopes required by the packaged agent; add narrow plugin scopes before enabling new plugins. |
| ABZ-DESK-SHELL-001 | critical | pass | Tauri shell plugin is not linked | Keep shell plugin disabled, or add explicit command and protocol allowlists before enabling it. |
| ABZ-DESK-OPENER-001 | high | pass | External opener calls use configured web URLs only | Never pass raw deep-link, remote, or user-controlled URLs into opener or shell-like APIs. |
| ABZ-DESK-TAURI-001 | critical | pass | Tauri core is at the local-origin confusion patch floor | Upgrade Tauri core to 2.11.1 or newer and refresh the lockfile. |
| ABZ-DESK-CSP-001 | high | pass | Packaged Tauri agent has an explicit CSP | Set a restrictive CSP for packaged builds and avoid remote/iframe content unless separately isolated. |
| ABZ-DESK-DEEPLINK-002 | high | pass | Deep-link auth parser rejects non-auth hosts and paths | Keep strict URL parsing and reject malformed, duplicated, oversized, or command-like deep-link inputs. |
| ABZ-DESK-DEEPLINK-003 | high | pass | Deep-link parser rejects oversized, duplicated, and command-like auth inputs | Keep the deep-link parser narrow and reject inputs that look like commands, files, URLs, or repeated auth fields. |
| ABZ-DESK-SECRETS-001 | critical | pass | Desktop credentials use OS keyring storage and audit redaction | Keep hw_secret, access tokens, and refresh tokens in keyring/DPAPI-backed storage and out of logs/files. |
| ABZ-DESK-HELPER-001 | critical | pass | Privileged helper install path requires trusted signed source and allowlisted actions | Gate privileged helper install/actions behind explicit UAC, signed binaries, trusted install paths, and action allowlists. |
| ABZ-DESK-LAB-001 | high | pass | Windows high-risk local checks are documented as lab-only validations | Keep unsafe local attack classes in a disposable Windows VM guide, not automated exploit tests. |

## Remediation Checklist

- [x] No failing desktop security findings in this run.
