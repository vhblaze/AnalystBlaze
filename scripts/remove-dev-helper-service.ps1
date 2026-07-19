param(
  [string]$TargetDir = "$env:LOCALAPPDATA\AnalystBlaze\cargo-target"
)

$ErrorActionPreference = "Stop"

function Test-IsAdmin {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-TargetPrefix {
  param([string]$Path)

  $resolved = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
  if ($resolved) {
    return $resolved.Path.TrimEnd('\') + '\'
  }
  return $Path.TrimEnd('\') + '\'
}

if (-not (Test-IsAdmin)) {
  $scriptPath = $PSCommandPath
  $arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$scriptPath`" -TargetDir `"$TargetDir`""
  Start-Process powershell -Verb RunAs -Wait -ArgumentList $arguments
  exit $LASTEXITCODE
}

$service = Get-CimInstance Win32_Service -Filter "Name='AnalystBlazeHelper'" -ErrorAction SilentlyContinue
if (-not $service) {
  Write-Host "AnalystBlazeHelper is not installed."
  exit 0
}

$targetPrefix = Get-TargetPrefix -Path $TargetDir
$pathName = [string]$service.PathName
$pointsToDevTarget =
  $pathName.IndexOf($targetPrefix, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
  $pathName.IndexOf("\cargo-target\debug\analystblaze-desktop.exe", [System.StringComparison]::OrdinalIgnoreCase) -ge 0

if (-not $pointsToDevTarget) {
  Write-Warning "AnalystBlazeHelper does not point to the dev target. Refusing to remove a possible production helper."
  Write-Warning "Current PathName: $pathName"
  exit 1
}

Write-Host "Stopping dev AnalystBlazeHelper service..."
sc.exe stop AnalystBlazeHelper | Out-Null
Start-Sleep -Milliseconds 1200

Write-Host "Deleting dev AnalystBlazeHelper service..."
sc.exe delete AnalystBlazeHelper | Out-Null
Start-Sleep -Milliseconds 1200

$remaining = Get-CimInstance Win32_Service -Filter "Name='AnalystBlazeHelper'" -ErrorAction SilentlyContinue
if ($remaining) {
  Write-Warning "Service deletion was requested, but Windows still reports it. Reboot if it remains marked for deletion."
  exit 1
}

Write-Host "Dev AnalystBlazeHelper service removed."
