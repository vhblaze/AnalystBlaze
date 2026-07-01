# Windows Lab-Only Validation Guide

Use this guide only inside a disposable Windows VM snapshot. Do not run these checks on production or personal machines that you cannot roll back.

## Safety Setup

1. Create a VM snapshot before installing AnalystBlaze.
2. Use fake AnalystBlaze accounts and non-production endpoints.
3. Disconnect real cloud credentials and payment keys.
4. Keep all marker files inside a temporary lab directory.
5. Roll back the VM snapshot after validation.

## Optional Guarded Script

The repo includes `scripts/security-lab-validation.ps1` for benign inspection only. It refuses to run unless all guards are set:

```powershell
$env:ABZ_SECURITY_MODE = "lab"
$env:ABZ_REQUIRE_VM_SNAPSHOT = "1"
$env:ABZ_SKIP_DESTRUCTIVE = "0"
powershell -ExecutionPolicy Bypass -File scripts/security-lab-validation.ps1
```

The script writes `reports/windows-lab-validation.json` and creates only inert marker files under `%TEMP%`.

## Checks

| Area | Benign Validation | Expected Result | Cleanup |
|---|---|---|---|
| service ACL | Inspect `AnalystBlazeHelper` with `sc.exe sdshow AnalystBlazeHelper` and `Get-Acl` on the helper directory. | SYSTEM/Admins can modify service files; normal user only has required queue permissions. | No mutation required. |
| helper path | Inspect `sc.exe qc AnalystBlazeHelper`. | Binary path points to the signed AnalystBlaze executable under Program Files/per-machine install root. | No mutation required. |
| signed artifact | Use `Get-AuthenticodeSignature` on the desktop exe and helper source binary. | Status is `Valid`; publisher matches expected release certificate. | No mutation required. |
| DLL search-order | In a purpose-built disposable test harness directory, place a harmless marker DLL next to an intentionally vulnerable lab binary, never the AnalystBlaze production binary. | Detection telemetry or EDR policy records the suspicious module-load condition. | Delete the harness directory or roll back snapshot. |
| service config drift | In a lab clone only, change a fake test service binary path, not AnalystBlazeHelper. | Monitoring detects service path drift. | Restore test service or roll back snapshot. |
| unsigned driver blocking | Attempt to load only a benign unsigned test driver if your lab policy allows it. | Windows vulnerable/unsigned driver policy blocks loading. | Remove the test driver and roll back snapshot. |
| process/module telemetry | Run a benign simulator that creates expected process/module telemetry without injection or credential access. | AnalystBlaze and endpoint logging record process/module metadata without collecting secrets. | Stop simulator and delete marker files. |

## Explicit Non-Goals

- No UAC bypass reproduction.
- No credential dumping.
- No kernel exploit or vulnerable-driver exploitation.
- No malicious process injection.
- No persistence on non-lab systems.
