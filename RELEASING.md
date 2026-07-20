# Releasing AnalystBlaze Desktop

This is the step-by-step for cutting a beta release of the desktop agent, from
version bump to the sanity checklist. The app currently ships **without
Authenticode code signing** (a deliberate cost decision for the beta phase -
see the hook described in [Authenticode (future)](#authenticode-future)). The
**Tauri updater signature is separate, free, and mandatory** - it's what lets
already-installed clients trust and auto-update to new builds, and it's
covered in detail below.

## 0. One-time setup: the updater signing key

Before the very first release, generate the updater key pair once:

```sh
npx tauri signer generate -w ~/.tauri/analystblaze-updater.key
```

This prints a **public key** and writes the **private key** to the given
path (protect it with the password prompt - don't use `--ci` unless you
immediately move the resulting secret into a password manager).

- **Public key** → paste it into `src-tauri/tauri.conf.json` at
  `plugins.updater.pubkey`, replacing the `TAURI_UPDATER_PUBKEY_AQUI`
  placeholder. This is safe to commit - it's only used to *verify* signatures,
  never to create them.
- **Private key** → **never commit it, never log it, never put it in a
  `.env` file that could be committed.** It lives only as GitHub Actions
  secrets:
  - `TAURI_SIGNING_PRIVATE_KEY` - the private key content (or path, per the
    Tauri CLI's own handling - see `tauri signer sign --help`).
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` - the password you set when
    generating it.
- **Back up the private key in two separate places** (e.g. a password
  manager vault AND an encrypted offline copy). **If this key is lost, every
  client that already has the app installed becomes permanently unable to
  auto-update** - they'd all need to be told to manually download and
  reinstall from scratch with a *new* key pair (which also requires shipping
  a build signed with the new key through some other channel, since the old
  clients can't verify it via auto-update either). Treat it like a production
  database credential, not a build artifact.

For local testing without touching the real key, generate a **separate test
key pair** the same way and point your local `tauri.conf.json` `pubkey` /
your local `TAURI_SIGNING_PRIVATE_KEY` at it. Never reuse the production key
for test builds.

## 1. Bump the version

The version lives in **three places** that must be bumped together - nothing
in this repo keeps them in sync automatically:

1. `src-tauri/tauri.conf.json` → top-level `"version"`. This is what Tauri
   uses to build `PackageInfo`, which is what the updater plugin compares
   against the server's manifest (`current_version`). It's also what the
   release CI workflow checks against the git tag.
2. `src-tauri/Cargo.toml` → `[package] version`. This drives
   `env!("CARGO_PKG_VERSION")`, which `src-tauri/src/updater.rs` uses for the
   post-restart "did the update actually apply" check and the
   `minimum_version` comparison, and which
   `src-tauri/src/optimizations/privileged_helper.rs` uses as the helper
   service's own version marker.
3. `src/i18n/translations.ts` → the `app.versionLine` string (shown in the
   sidebar footer, both `pt-BR` and `en-US`). Purely cosmetic, but stale text
   here confuses support conversations.

Keep all three identical (e.g. `0.2.0`). A mismatch between (1) and (2) means
the updater plugin's own idea of "current version" (tauri.conf.json) would
silently diverge from what the app's own update-decision logic uses
(Cargo.toml) - the release workflow only catches a mismatch against the git
tag, not between these two files, so double check by eye.

## 2. Write the changelog / release notes

Keep it short and honest - plain language, no "AI learned" marketing. This
text becomes the `notes` field in the update manifest and is shown verbatim
in the in-app update card. Put it wherever you keep release notes today (or
directly in the `notes` field of `release-manifest.json`, step 5 below).

## 3. Tag and push

```sh
git tag v0.2.0
git push origin v0.2.0
```

Pushing a `v*` tag triggers `.github/workflows/release.yml`.

## 4. Let CI build, sign, and draft the release

The workflow:

1. Checks the pushed tag matches `tauri.conf.json`'s version (fails loudly if
   you forgot step 1).
2. Runs `npx tauri build` with `TAURI_SIGNING_PRIVATE_KEY` /
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` set, which makes the Tauri CLI emit a
   `.sig` file next to the NSIS installer (`bundle.windows.nsis`, per-machine,
   with the `installer/hooks.nsh` service stop/restart hooks - see
   [Helper service handshake](#helper-service-handshake-during-updates)).
3. Computes the installer's SHA-256.
4. Generates `release-manifest.json` - a ready-to-paste body for the admin
   registration call in step 5, with `notes` left as a `TODO:` placeholder for
   you to fill in with the changelog from step 2.
5. Publishes a **draft** GitHub Release with the installer, `.sig`, sha256
   file, and `release-manifest.json` attached, and also uploads them as a
   workflow artifact (useful if you want to inspect them before the release
   even exists).

Watch the Actions run; if the tag/version check fails, delete the tag
(`git push --delete origin v0.2.0` / `git tag -d v0.2.0`), fix the version
files, and re-tag.

## 5. Publish the draft and register the release with the server

1. Open the draft release on GitHub, fill in `notes` with the real changelog,
   and publish it (un-draft).
2. Edit `release-manifest.json` (downloaded from the workflow artifact or the
   release assets) to replace the `notes` TODO with the same changelog text,
   and set `minimumVersion` if this release fixes a security issue or breaks
   compatibility with the server (leave it `null` otherwise - see
   [minimum_version](#minimum_version-semantics) below).
3. Register it with the server's admin endpoint:

   ```sh
   curl -X POST https://api.analystblaze.com/api/v1/admin/updates/releases \
     -H "X-AnalystBlaze-Releases-Token: $DESKTOP_RELEASES_ADMIN_TOKEN" \
     -H "Content-Type: application/json" \
     -d @release-manifest.json
   ```

   The endpoint upserts by `(platform, version)`, so re-running this after
   fixing a typo in `notes` is safe - it just overwrites that row.
4. Confirm it worked:

   ```sh
   curl "https://api.analystblaze.com/api/v1/updates/manifest?target=windows&arch=x86_64&current_version=0.0.0"
   ```

   should return your new release's manifest JSON (not 204).

### `minimum_version` semantics

If set, any installed client below that version gets the "update required"
tone in the UI (still asks for consent - see
[UX contract](#update-ux-contract), never force-installs). Use it for
security fixes or server-breaking changes only; leave it `null` for routine
releases.

## 6. Update the web download wizard's env vars

The web app's download/install wizard (`AnalystBlaze-web`,
`DownloadAgentView.tsx`) reads the installer URL/version/channel from
`NEXT_PUBLIC_AGENT_INSTALLER_URL`, `NEXT_PUBLIC_AGENT_INSTALLER_VERSION`,
`NEXT_PUBLIC_AGENT_RELEASE_CHANNEL`, and `NEXT_PUBLIC_AGENT_RELEASE_NOTES_URL`.
Update these in the Vercel deploy to point at the new release. The SHA-256
(`NEXT_PUBLIC_AGENT_INSTALLER_SHA256`, from `release-manifest.json`'s `sha256`
field or the `.sha256` asset) is no longer shown in the end-user UI - it's
kept only for the local install-consent record - but keep it in sync anyway
since it's cheap and future UI may want it back.

## 7. Sanity checklist

Do this against the real server (or a staging copy) before telling anyone the
release is out:

- [ ] Install the **previous** released version fresh (or keep an existing
      install around).
- [ ] Launch it, confirm the app is on the old version (Settings → check
      current version).
- [ ] Register the new release with the server (step 5) if not already done.
- [ ] Click "Verificar atualizacoes" in Settings, or wait for the periodic
      background check - confirm the update card appears with the right
      version and notes.
- [ ] Click "Atualizar agora" (or "Depois" first, then come back and click it
      from the persistent badge) and confirm the app restarts on the new
      version.
- [ ] Open Historico/Auditoria and confirm `update.detected`,
      `update.download_completed`, `update.install_started`, and
      `update.installed_successfully` events are all present with sane
      timestamps.
- [ ] If the privileged helper is installed, confirm its status still shows
      as available (or auto-recovers after a restart) - see
      [Helper service handshake](#helper-service-handshake-during-updates).
- [ ] Deep-link pairing (`analystblaze://auth`) still opens login correctly
      after the update (confirms the deep-link registration survived the
      per-machine reinstall).

## Update UX contract

Read `src-tauri/src/updater.rs` and `src/components/analystblaze/UpdateNotice.tsx`
if you need to change this, but the intended behavior is:

- Background checks run once ~4 minutes after startup, then every ~8 hours.
  Manual checks are available in Settings.
- Detecting an update never installs anything by itself. It's allowed to
  **download** the package in the background so "Atualizar agora" is instant,
  but installation only ever happens after the user clicks that button.
- "Depois" hides the prompt for 24h; availability stays visible passively
  (a badge in Settings) during that window.
- A signature that fails verification is discarded, logged as a security
  event (`update.signature_invalid`), and shown to the user in plain language
  - never installed silently, never a raw crash.
- Network/check failures are logged and retried next cycle - never a
  user-facing error.

## Helper service handshake during updates

The privileged helper (`AnalystBlazeHelper` Windows service) is the **same
executable** as the main app, just launched with
`--analystblaze-helper-service`. That means an app update overwrites the
service's own binary. Two things exist specifically to keep this safe:

1. `src-tauri/installer/hooks.nsh` stops the service before the installer
   overwrites files and restarts it after, via NSIS's
   `NSIS_HOOK_PREINSTALL` / `NSIS_HOOK_POSTINSTALL` (wired in
   `tauri.conf.json`'s `bundle.windows.nsis.installerHooks`). Without this, a
   running service can hold the exe locked during install, and even if the
   overwrite succeeds, the *already-running* old process would keep serving
   requests with old code until manually restarted.
2. Every helper IPC request carries a `protocol_version`
   (`optimizations/privileged_helper.rs::REQUEST_PROTOCOL_VERSION`). If the
   app and the running helper disagree, the helper rejects the request; the
   app recognizes that specific rejection and downgrades the error message to
   the same "helper needs a restart" guidance used elsewhere (see
   `degraded_helper_message()`), instead of surfacing a raw protocol error.
   `privileged_helper::status()` also proactively reports `requiresUpdate`
   with a friendly message when the on-disk helper version differs from the
   running app's version, even before any action is attempted.

If you ever split the helper into a separate binary from the main app,
revisit both of these - the NSIS hook path assumes one exe, and the
version-file comparison assumes both binaries always share
`CARGO_PKG_VERSION`.

## Authenticode (future)

Out of scope for now (cost), but the hook is ready: add
`bundle.windows.signCommand` to `src-tauri/tauri.conf.json` (see
`WindowsConfig` in the [Tauri v2 config schema](https://schema.tauri.app/config/2))
once a certificate is purchased, and uncomment the `WINDOWS_CERTIFICATE` /
`WINDOWS_CERTIFICATE_PASSWORD` secrets referenced (commented out) in
`.github/workflows/release.yml`. No other workflow or docs change should be
needed.

Note: `scripts/tauri-build-sac.ps1` (used by `npm run desktop:build` locally)
self-signs Cargo outputs with a throwaway local dev certificate purely so the
privileged helper's `Get-AuthenticodeSignature` trust check passes during
local development. That is **not** a substitute for Authenticode and must
never be used for CI/production builds - the release workflow calls
`npx tauri build` directly, not that script.
