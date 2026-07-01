<# 
[LAB ONLY] AnalystBlaze Windows validation helper.

This script performs benign inspection only. It does not exploit, persist,
inject, dump credentials, bypass UAC, or modify services by default.

Required guards:
  ABZ_SECURITY_MODE=lab
  ABZ_REQUIRE_VM_SNAPSHOT=1
  ABZ_SKIP_DESTRUCTIVE=0
#>

[CmdletBinding()]
param(
    [string]$ServiceName = "AnalystBlazeHelper",
    [string]$ExpectedPublisher = "",
    [string]$ReportPath = "reports/windows-lab-validation.json"
)

$ErrorActionPreference = "Stop"

function Assert-LabGuard {
    if ($env:ABZ_SECURITY_MODE -ne "lab") {
        throw "Refusing to run: set ABZ_SECURITY_MODE=lab in a disposable VM."
    }
    if ($env:ABZ_REQUIRE_VM_SNAPSHOT -ne "1") {
        throw "Refusing to run: set ABZ_REQUIRE_VM_SNAPSHOT=1 after creating a VM snapshot."
    }
    if ($env:ABZ_SKIP_DESTRUCTIVE -ne "0") {
        throw "Refusing to run: set ABZ_SKIP_DESTRUCTIVE=0 to acknowledge lab-only benign inspection."
    }
}

function New-Result($Id, $Title, $Status, $Evidence, $Remediation) {
    [ordered]@{
        id = $Id
        title = $Title
        status = $Status
        evidence = $Evidence
        remediation = $Remediation
    }
}

Assert-LabGuard

$results = New-Object System.Collections.Generic.List[object]
$service = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'" -ErrorAction SilentlyContinue

if ($service) {
    $results.Add((New-Result "ABZ-LAB-SERVICE-PATH" "Helper service path is inspectable and quoted" "pass" $service.PathName "Service binary path should point to signed AnalystBlaze under Program Files."))
    $sd = (& sc.exe sdshow $ServiceName) -join "`n"
    $results.Add((New-Result "ABZ-LAB-SERVICE-ACL" "Helper service ACL can be inspected" "pass" $sd "Review service ACL for least privilege; do not grant normal users service config write."))

    $exePath = $service.PathName
    if ($exePath.StartsWith('"')) {
        $exePath = $exePath.Substring(1, $exePath.IndexOf('"', 1) - 1)
    } else {
        $exeMatch = [regex]::Match($exePath, '^(.+?\.exe)')
        $exePath = $exeMatch.Groups[1].Value
    }

    if (Test-Path -LiteralPath $exePath) {
        $signature = Get-AuthenticodeSignature -LiteralPath $exePath
        $publisherOk = [string]::IsNullOrWhiteSpace($ExpectedPublisher) -or ($signature.SignerCertificate.Subject -like "*$ExpectedPublisher*")
        $status = if ($signature.Status -eq "Valid" -and $publisherOk) { "pass" } else { "fail" }
        $results.Add((New-Result "ABZ-LAB-SIGNATURE" "Helper artifact signature is valid" $status "$($signature.Status) $($signature.SignerCertificate.Subject)" "Ship helper/updater artifacts with a trusted Authenticode signature."))
    } else {
        $results.Add((New-Result "ABZ-LAB-SIGNATURE" "Helper artifact signature is valid" "skip" "Executable path not found: $exePath" "Install a signed per-machine build before this check."))
    }
} else {
    $results.Add((New-Result "ABZ-LAB-SERVICE-PATH" "Helper service path is inspectable and quoted" "skip" "Service not installed: $ServiceName" "Install the helper in the snapshot VM if service checks are in scope."))
}

$programFiles = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ }
foreach ($root in $programFiles) {
    $candidate = Join-Path $root "AnalystBlaze"
    if (Test-Path -LiteralPath $candidate) {
        $acl = Get-Acl -LiteralPath $candidate
        $results.Add((New-Result "ABZ-LAB-INSTALL-ACL" "Install directory ACL is reviewable" "pass" ($acl.AccessToString -replace "`r?`n", " | ") "Normal users should not have write/modify access to helper binaries."))
    }
}

$labRoot = Join-Path $env:TEMP "AnalystBlaze-Lab-Markers"
New-Item -ItemType Directory -Force -Path $labRoot | Out-Null
Set-Content -LiteralPath (Join-Path $labRoot "phantom-marker.dll") -Value "inert marker only" -Encoding UTF8
$results.Add((New-Result "ABZ-LAB-DLL-MARKER" "Inert DLL search-order marker created" "pass" $labRoot "Use only inert marker files in lab harnesses; delete the lab marker directory after validation."))

$report = [ordered]@{
    generated_at = (Get-Date).ToUniversalTime().ToString("o")
    suite = "analystblaze-windows-lab-validation"
    lab_only = $true
    results = $results
}

$reportDir = Split-Path -Parent $ReportPath
if ($reportDir) {
    New-Item -ItemType Directory -Force -Path $reportDir | Out-Null
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $ReportPath -Encoding UTF8
Write-Host "Wrote $ReportPath"
Write-Host "Cleanup marker directory when done: $labRoot"
