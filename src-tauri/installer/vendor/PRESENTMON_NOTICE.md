# PresentMon - vendored third-party binary

`PresentMon-2.5.1-x64.exe` in this directory is the **standalone PresentMon
Console Application**, built and published by Intel Corporation as part of
the open-source [GameTechDev/PresentMon](https://github.com/GameTechDev/PresentMon)
project. It captures GPU present events via ETW (Event Tracing for Windows)
and writes frame-time data to stdout/CSV - see
`optimizations::frame_capture_control` for how AnalystBlaze spawns it, and
`telemetry::frame_capture` for how its output is parsed.

This is the small (~950KB) console-only executable, not the ~150MB
service+GUI+overlay MSI installer the same project also publishes - we only
need one-shot capture, not the persistent background service.

## Provenance (verified 2026-09-06)

- **Source**: https://github.com/GameTechDev/PresentMon/releases/tag/v2.5.1
- **Asset**: `PresentMon-2.5.1-x64.exe`, downloaded directly from GitHub's
  `releases/download` CDN over HTTPS (no third-party mirror).
- **SHA256** (as downloaded, computed by us - GameTechDev does not publish
  its own checksum file for this release):
  `9bec3083069f58f911e6a512f4806db51a27bd096103087bc1d05ef54c80a191`
- **Authenticode signature** (verified via `Get-AuthenticodeSignature` -
  this is the stronger check, since it's independently issued by a public
  CA rather than something we or GameTechDev computed ourselves):
  - Status: `Valid` ("Signature verified.")
  - Subject: `CN=Intel Corporation, O=Intel Corporation, S=California, C=US`
  - Issuer: `CN=Sectigo Public Code Signing CA R36, O=Sectigo Limited, C=GB`
  - Validity: 2025-08-09 to 2026-08-10
  - Thumbprint: `4B923D748E9EBE27252FDBA244342C1888A2D23E`

## Behavior relevant to bundling this into AnalystBlaze

- No network access, telemetry, or "phone home" behavior found in the
  console application's documented behavior - it's a local ETW consumer
  that writes to stdout/a file. (The separate, much larger "PresentMon
  Service" - not vendored here - aggregates hardware telemetry from vendor
  APIs like NVAPI for its own local GUI/overlay clients over local named
  shared memory; still not a network call, but we don't ship that
  component at all.)
- Capturing reliably needs the caller to either be elevated or be a member
  of "Performance Log Users" - this is exactly why AnalystBlaze only spawns
  it from the privileged helper service (see
  optimizations::privileged_helper), never from the main app process.

## License

MIT (`Copyright (C) 2017-2024 Intel Corporation`) - permits redistributing
the compiled binary in a closed-source product, with no obligation beyond
keeping the copyright and permission notice. Full text:

```
Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the "Software"),
to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS," without warranty of any kind, express or
implied, including but not limited to the warranties of merchantability,
fitness for a particular purpose and noninfringement. In no event shall the
authors or copyright holders be liable for any claim, damages or other
liability, whether in an action of contract, tort or otherwise, arising
from, out of or in connection with the software or the use or other
dealings in the software.
```

## Upgrading this binary later

Never silently swap this file for "whatever the latest release is." Repeat
the same steps: download the exact new version's console-app `.exe` from
the official GitHub releases page, verify its Authenticode signature is
still `Valid` and still signed by Intel Corporation, record the new
version/SHA256/thumbprint here, then replace the file - as its own
reviewable change, not bundled into an unrelated commit.
