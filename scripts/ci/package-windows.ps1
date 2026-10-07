param(
    [Parameter(Mandatory=$true)]
    [string]$Arch,
    [Parameter(Mandatory=$true)]
    [string]$Version
)

$ErrorActionPreference = "Stop"

New-Item -ItemType Directory -Force out | Out-Null
$out = (Resolve-Path out).Path

Copy-Item README.md, LICENSE, NOTICE dist/windows/
Copy-Item -Recurse -Force LICENSES dist/windows/
New-Item -ItemType Directory -Force dist/server | Out-Null
Copy-Item dist/windows/neoomsi.exe, dist/windows/vcruntime140.dll, dist/windows/vcruntime140_1.dll, dist/windows/msvcp140.dll, scripts/server/start.cmd, LICENSE, NOTICE dist/server/
Copy-Item -Recurse -Force LICENSES dist/server/
Copy-Item docs/SERVER.md dist/server/README.md
New-Item -ItemType File -Force dist/server/.neocontent | Out-Null

Push-Location dist/windows
7z a -tzip -mx=5 "$out/neoOMSI-$Version-windows-$Arch.zip" *
Pop-Location

Push-Location dist/server
7z a -tzip -mx=5 "$out/neoOMSI-$Version-server-windows-$Arch.zip" *
Pop-Location
