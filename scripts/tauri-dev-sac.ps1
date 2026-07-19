param(
  [string]$TargetDir = "$env:LOCALAPPDATA\AnalystBlaze\cargo-target",
  [int]$MaxAttempts = 6
)

$ErrorActionPreference = "Stop"

$root = Resolve-Path "$PSScriptRoot\.."
$certSubject = "CN=AnalystBlaze Local Rust Build Signing"

function Get-OrCreateBuildCertificate {
  $cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $certSubject } |
    Sort-Object NotAfter -Descending |
    Select-Object -First 1

  if (-not $cert) {
    $cert = New-SelfSignedCertificate `
      -Type CodeSigningCert `
      -Subject $certSubject `
      -CertStoreLocation Cert:\CurrentUser\My `
      -KeyUsage DigitalSignature `
      -KeyExportPolicy Exportable `
      -HashAlgorithm SHA256
  }

  foreach ($storeName in @("Root", "TrustedPublisher")) {
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName, "CurrentUser")
    $store.Open("ReadWrite")
    try {
      $exists = $store.Certificates | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
      if (-not $exists) {
        $store.Add($cert)
      }
    } finally {
      $store.Close()
    }
  }

  return $cert
}

function Sign-CargoOutputs {
  param(
    [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate,
    [string]$Path
  )

  if (-not (Test-Path $Path)) {
    return 0
  }

  $signed = 0
  $files = Get-ChildItem -Path $Path -Recurse -Include *.dll,*.exe -File -ErrorAction SilentlyContinue
  foreach ($file in $files) {
    try {
      $signature = Get-AuthenticodeSignature -FilePath $file.FullName
      if ($signature.Status -eq "Valid") {
        continue
      }

      $result = Set-AuthenticodeSignature -FilePath $file.FullName -Certificate $Certificate -HashAlgorithm SHA256
      if ($result.Status -eq "Valid") {
        $signed++
      }
    } catch {
      Write-Warning "Skipping locked binary while signing: $($file.FullName) ($($_.Exception.Message))"
    }
  }

  return $signed
}

function Stop-RunningDevBinary {
  param([string]$Path)

  $resolvedTarget = (Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue)
  $targetPrefix = if ($resolvedTarget) { $resolvedTarget.Path.TrimEnd('\') + '\' } else { $Path.TrimEnd('\') + '\' }
  $service = Get-CimInstance Win32_Service -Filter "Name='AnalystBlazeHelper'" -ErrorAction SilentlyContinue
  if ($service -and $service.PathName -and $service.PathName.IndexOf($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
    Write-Warning "AnalystBlazeHelper is installed from the dev target and can lock rebuilds. Remove it from an elevated PowerShell: sc.exe stop AnalystBlazeHelper; sc.exe delete AnalystBlazeHelper"
  }

  $processes = Get-CimInstance Win32_Process -Filter "Name='analystblaze-desktop.exe'" -ErrorAction SilentlyContinue

  foreach ($process in $processes) {
    $exePath = [string]$process.ExecutablePath
    $commandLine = [string]$process.CommandLine
    $fromTargetDir =
      ($exePath -and $exePath.StartsWith($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase)) -or
      ($commandLine -and $commandLine.IndexOf($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)

    # SAC can hide ExecutablePath/CommandLine for the dev binary. In this script,
    # the package executable name is specific enough to treat a blank path as stale dev state.
    if (-not $fromTargetDir -and ($exePath -or $commandLine)) {
      continue
    }

    Write-Host "Stopping locked AnalystBlaze dev process PID $($process.ProcessId)."
    Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
    try {
      Wait-Process -Id $process.ProcessId -Timeout 10 -ErrorAction SilentlyContinue
    } catch {}
    if (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue) {
      Write-Warning "PID $($process.ProcessId) is still running. If it is owned by AnalystBlazeHelper, stop/delete that service from an elevated PowerShell."
    }
  }
}

New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
$cert = Get-OrCreateBuildCertificate
$env:CARGO_TARGET_DIR = $TargetDir

Push-Location $root
try {
  for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
    Write-Host "tauri dev attempt $attempt/$MaxAttempts using target $TargetDir"
    Stop-RunningDevBinary -Path $TargetDir
    npx tauri dev
    if ($LASTEXITCODE -eq 0) {
      exit 0
    }

    Stop-RunningDevBinary -Path $TargetDir
    $signed = Sign-CargoOutputs -Certificate $cert -Path $TargetDir
    Write-Host "Signed $signed generated Cargo binaries after failed attempt."
    if ($signed -eq 0) {
      exit $LASTEXITCODE
    }
  }

  Write-Error "tauri dev did not start after $MaxAttempts attempts."
  exit 1
} finally {
  Pop-Location
}
