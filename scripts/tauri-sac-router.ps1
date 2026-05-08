param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$TauriArgs
)

$ErrorActionPreference = "Stop"

$command = if ($TauriArgs.Count -gt 0) { $TauriArgs[0] } else { "" }
$remaining = @()
if ($TauriArgs.Count -gt 1) {
  $remaining = $TauriArgs[1..($TauriArgs.Count - 1)]
}

switch ($command) {
  "dev" {
    if ($remaining.Count -gt 0) {
      & "$PSScriptRoot\tauri-dev-sac.ps1" @remaining
    } else {
      & "$PSScriptRoot\tauri-dev-sac.ps1"
    }
    exit $LASTEXITCODE
  }
  "build" {
    if ($remaining.Count -gt 0) {
      & "$PSScriptRoot\tauri-build-sac.ps1" @remaining
    } else {
      & "$PSScriptRoot\tauri-build-sac.ps1"
    }
    exit $LASTEXITCODE
  }
  default {
    & npx tauri @TauriArgs
    exit $LASTEXITCODE
  }
}
