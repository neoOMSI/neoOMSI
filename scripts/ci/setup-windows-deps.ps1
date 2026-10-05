param(
    [Parameter(Mandatory=$true)]
    [string]$Arch
)

$ErrorActionPreference = "Stop"

try {
    gh release download -R microsoft/DirectXShaderCompiler --pattern "dxc_*.zip" -D dxc
    $zip = Get-ChildItem dxc/*.zip | Where-Object { $_.Name -notmatch "linux|pdb" } | Select-Object -First 1
    Expand-Archive $zip.FullName -DestinationPath dxc/x
    $dxcArch = if ($Arch -eq "arm64") { "arm64" } else { "x64" }
    Copy-Item "dxc/x/bin/$dxcArch/dxcompiler.dll", "dxc/x/bin/$dxcArch/dxil.dll" dist/windows/
} catch {
    Write-Warning "DirectX shader compiler setup failed: $_"
}

$vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -property installationPath
$crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\$Arch\Microsoft.VC*.CRT" -Directory -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
$dir = if ($crt) { $crt.FullName } elseif ($Arch -eq "x64") { "$env:SystemRoot\System32" } else { throw "no ARM64 Visual C++ runtime in $vs" }
foreach ($dll in "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll") {
    Copy-Item (Join-Path $dir $dll) dist/windows/
}
