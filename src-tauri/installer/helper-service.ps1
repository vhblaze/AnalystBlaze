# AnalystBlazeHelper service management (install / uninstall).
#
# This is the SINGLE canonical script that registers the privileged Windows
# service used for admin-gated optimizations. It is invoked from two places:
#
#   1. The NSIS installer hooks (installer/hooks.nsh) during a per-machine
#      install/uninstall. NSIS already runs elevated in per-machine mode, so no
#      extra UAC prompt is raised here.
#   2. The in-app "Reparar helper" flow (optimizations/privileged_helper.rs),
#      which elevates just this action via UAC when the app is not admin.
#
# The service points at the SAME app executable launched with the
# `--analystblaze-helper-service` flag (see main.rs). The IPC signing key and
# version.txt live under %ProgramData%\AnalystBlaze\Helper; the running service
# creates the signing key on first start (ensure_helper_signing_key_file) and
# writes version.txt, so this script only has to prepare the directory ACLs and
# register/start the service.
#
# ACL note: the helper root is granted read/execute to BUILTIN\Users so the
# interactive desktop app (running as the logged-in user) can read the signing
# key the SYSTEM service creates. The installer cannot know the interactive
# user's SID reliably, and read access alone is harmless: admin actions are
# still gated by the named-pipe ACL, the signed-client-exe check, a fresh nonce,
# a short expiry, and explicit local confirmation on every request.

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('install', 'uninstall')]
    [string]$Action,

    [string]$ExePath
)

$ErrorActionPreference = 'Stop'

$ServiceName = 'AnalystBlazeHelper'
$DisplayName = 'AnalystBlaze Privileged Helper'
$Description = 'Executes AnalystBlaze local privileged actions after app-side confirmation.'
$HelperRoot = Join-Path $env:ProgramData 'AnalystBlaze\Helper'

function Remove-HelperService {
    $svc = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($svc) {
        & sc.exe stop $ServiceName | Out-Null
        Start-Sleep -Milliseconds 800
        & sc.exe delete $ServiceName | Out-Null
        Start-Sleep -Milliseconds 800
    }
}

if ($Action -eq 'uninstall') {
    Remove-HelperService
    if (Test-Path -LiteralPath $HelperRoot) {
        Remove-Item -LiteralPath $HelperRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    exit 0
}

# --- install ---------------------------------------------------------------

if ([string]::IsNullOrWhiteSpace($ExePath) -or -not (Test-Path -LiteralPath $ExePath)) {
    Write-Error "Executavel do helper nao encontrado: $ExePath"
    exit 1
}

New-Item -ItemType Directory -Force -Path $HelperRoot | Out-Null

# SYSTEM + Administrators: full control. BUILTIN\Users: read/execute (inherited
# by the signing key the service creates on first start).
& icacls $HelperRoot /inheritance:r `
    /grant:r '*S-1-5-18:(OI)(CI)F' `
    '*S-1-5-32-544:(OI)(CI)F' `
    '*S-1-5-32-545:(OI)(CI)RX' | Out-Null

Remove-HelperService

$binPath = '"{0}" --analystblaze-helper-service' -f $ExePath
& sc.exe create $ServiceName binPath= $binPath start= auto DisplayName= $DisplayName | Out-Null
& sc.exe description $ServiceName $Description | Out-Null
& sc.exe start $ServiceName | Out-Null

exit 0
