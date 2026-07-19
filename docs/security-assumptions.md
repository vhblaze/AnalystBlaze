# Desktop Security Assumptions

- The Tauri app is packaged from `src-tauri`.
- The desktop agent should not render untrusted remote pages or iframes with privileged IPC access.
- The shell plugin is not required for normal AnalystBlaze workflows.
- Deep links are used only for AnalystBlaze authentication callbacks.
- Deep-link inputs must stay below the parser limit and must not carry shell/path/open command parameters or duplicated auth fields.
- Local secret material is expected to remain in platform-protected storage and must not be exposed through deep links, logs, or command-line arguments.
- Privileged helper installation is expected only from a trusted Program Files/per-machine path with a valid Authenticode signature.
- Local optimization actions must be documented in `docs/optimization-safety-matrix.md` before release, including risk, snapshot behavior, restore path, and irreversible cases.
- Reversible actions are expected to create local snapshots before mutating Windows state, or to move files into cleanup quarantine instead of deleting them immediately.
- Irreversible actions are expected to require explicit local confirmation and must not trust caller-provided filesystem paths as deletion authority.
- TPM-backed device attestation is not verified by this harness; if it is not implemented, device clone resistance remains a documented product gap.
