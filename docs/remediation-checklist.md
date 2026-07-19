# Desktop Security Remediation Checklist

- [x] Upgrade Tauri core to `2.11.1` or newer and refresh `src-tauri/Cargo.lock`.
- [x] Replace `app.security.csp: null` in `src-tauri/tauri.conf.json` with an explicit restrictive CSP before production packaging.
- [x] Keep `tauri-plugin-shell` disabled unless a narrow allowlist and regression tests are added.
- [x] Keep opener/deep-link flows limited to configured login/account URLs and strict `analystblaze://auth` callbacks.
- [ ] Add TPM or platform-backed non-exportable device-key validation if strong clone resistance becomes a release requirement.
- [x] Keep privileged Windows behavior behind manual, lab-only benign validation rather than automated exploit reproduction.
- [x] Document reversible, irreversible, sensitive, and blocked optimization actions in `docs/optimization-safety-matrix.md`.
- [ ] Add a release gate that fails when a new supported optimization action is missing from the safety matrix.
