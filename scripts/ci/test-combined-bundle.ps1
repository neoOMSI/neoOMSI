# End-to-end validation of the combined engine + pinned launcher package
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$LauncherSrc = "",
    [string]$DistDir = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
Set-Location $root

Write-Host "=== Combined Bundle Check ===" -ForegroundColor Cyan

# 1. Validate pinned launcher reference
Write-Host "Step 1: Validating scripts/launcher-ref..." -ForegroundColor Yellow
$ref = if ($env:LAUNCHER_SHA) { $env:LAUNCHER_SHA.Trim() } else {
    $refFile = Join-Path $root "scripts/launcher-ref"
    if (-not (Test-Path $refFile)) {
        throw "scripts/launcher-ref does not exist"
    }
    (Get-Content $refFile).Trim()
}
if ($ref -notmatch '^[0-9a-f]{40}$') {
    throw "Launcher ref must be a 40-character hex commit SHA, found: '$ref'"
}
Write-Host "Pinned launcher SHA: $ref"

if ($DistDir -ne "") {
    $distWin = (Resolve-Path $DistDir).Path
} else {
    $distWin = Join-Path $root "dist/windows"
}

if (-not $SkipBuild) {
    # 2. Build engine and protocol check binaries
    Write-Host "`nStep 2: Building engine and protocol-check binaries..." -ForegroundColor Yellow
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    cargo build --locked -p core -p legacy-launcher-core -p launcher-protocol --bin neoomsi --bin neoomsi-launcher --bin protocol-check
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed"
    }

    # 3. Assemble engine distribution folder
    Write-Host "`nStep 3: Assembling distribution directory..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force $distWin | Out-Null
    Copy-Item -Force "target/debug/neoomsi.exe" (Join-Path $distWin "neoomsi.exe")
    Copy-Item -Force "target/debug/neoomsi-launcher.exe" (Join-Path $distWin "neoomsi-launcher.exe")

    # 4. Build and package pinned launcher
    Write-Host "`nStep 4: Building and packaging pinned launcher..." -ForegroundColor Yellow
    $bash = "C:\Program Files\Git\bin\bash.exe"
    if (-not (Test-Path $bash)) {
        $bash = "C:\Program Files (x86)\Git\bin\bash.exe"
    }
    if (-not (Test-Path $bash)) {
        $bashCmd = Get-Command bash -ErrorAction SilentlyContinue
        if ($bashCmd) { $bash = $bashCmd.Source }
    }
    if (-not (Test-Path $bash)) {
        throw "Git Bash is required to run build-launcher.sh"
    }

    $buildLauncherSh = (Join-Path $root "scripts/ci/build-launcher.sh").Replace('\', '/')
    if ($LauncherSrc -ne "") {
        $env:LAUNCHER_SRC = $LauncherSrc
    }
    & $bash $buildLauncherSh "windows" "x64"
    if ($LASTEXITCODE -ne 0) {
        throw "build-launcher.sh failed with exit code $LASTEXITCODE"
    }
} else {
    Write-Host "`nSteps 2-4: Skipping build, verifying prebuilt bundle at $distWin..." -ForegroundColor Yellow
}

# 5. Verify assembled bundle layout and essential files
Write-Host "`nStep 5: Verifying assembled bundle layout..." -ForegroundColor Yellow
$requiredFiles = @(
    (Join-Path $distWin "neoomsi.exe"),
    (Join-Path $distWin "neoomsi-launcher.exe"),
    (Join-Path $distWin "launcher/neoOMSI Launcher.exe"),
    (Join-Path $distWin "launcher/resources")
)

foreach ($file in $requiredFiles) {
    if (-not (Test-Path $file)) {
        throw "Validation failed: required bundle component missing: $file"
    }
    Write-Host "  Found: $file" -ForegroundColor Green
}

# 6. Real process --control-protocol handshake validation
Write-Host "`nStep 6: Running real --control-protocol handshake..." -ForegroundColor Yellow
$protocolCheckExe = Join-Path $root "target/debug/protocol-check.exe"
if (-not (Test-Path $protocolCheckExe)) {
    $protocolCheckExe = Join-Path $root "target/release/protocol-check.exe"
}
if (-not (Test-Path $protocolCheckExe)) {
    Write-Host "Building protocol-check test binary..." -ForegroundColor Yellow
    cargo build --locked -p launcher-protocol --bin protocol-check
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build for protocol-check failed"
    }
    $protocolCheckExe = Join-Path $root "target/debug/protocol-check.exe"
}
$engineExe = Join-Path $distWin "neoomsi.exe"
& $protocolCheckExe $engineExe
if ($LASTEXITCODE -ne 0) {
    throw "Protocol handshake check failed with exit code $LASTEXITCODE"
}

# 6b. Verify Electron launcher's TypeScript ProcessEngineClient directly against engine
$launcherRepoDir = if ($LauncherSrc -ne "") {
    (Resolve-Path $LauncherSrc).Path
} elseif ($env:LAUNCHER_SRC) {
    (Resolve-Path $env:LAUNCHER_SRC).Path
} else {
    Join-Path $root "target/neoomsi-launcher-src"
}

# Explicit local sources may contain intentional uncommitted changes. All other
# integration tests must use exactly the pinned commit.
$localOverride = ($LauncherSrc -ne "") -or [bool]$env:LAUNCHER_SRC
if ($localOverride) {
    if (-not (Test-Path (Join-Path $launcherRepoDir "package.json"))) {
        throw "Explicit launcher source is missing package.json: $launcherRepoDir"
    }
} else {
    if (-not (Test-Path (Join-Path $launcherRepoDir "package.json"))) {
        if (Test-Path $launcherRepoDir) {
            throw "Launcher checkout directory exists but is incomplete: $launcherRepoDir"
        }
        Write-Host "Cloning launcher at commit $ref for integration test..." -ForegroundColor Yellow
        New-Item -ItemType Directory -Force $launcherRepoDir | Out-Null
        git init --quiet $launcherRepoDir
        if ($LASTEXITCODE -ne 0) { throw "Could not initialize launcher checkout" }
        git -C $launcherRepoDir fetch --quiet --depth 1 https://github.com/neoOMSI/launcher.git $ref
        if ($LASTEXITCODE -ne 0) { throw "Could not fetch launcher commit $ref" }
        git -C $launcherRepoDir checkout --quiet --detach FETCH_HEAD
        if ($LASTEXITCODE -ne 0) { throw "Could not check out launcher commit $ref" }
    }
    $actualSha = (& git -C $launcherRepoDir rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or $actualSha.Trim() -ne $ref) {
        throw "Launcher checkout at $launcherRepoDir does not match pinned SHA $ref (found: $actualSha)."
    }
    $dirty = (& git -C $launcherRepoDir status --porcelain)
    if ($LASTEXITCODE -ne 0 -or $dirty) {
        throw "Pinned launcher checkout at $launcherRepoDir has local modifications. Use -LauncherSrc for intentional local testing."
    }
}
Write-Host "Running launcher TypeScript ProcessEngineClient integration test in $launcherRepoDir..." -ForegroundColor Yellow
Push-Location $launcherRepoDir
$reportFile = [System.IO.Path]::GetTempFileName()
try {
    $packageManager = (Get-Content (Join-Path $launcherRepoDir "package.json") -Raw | ConvertFrom-Json).packageManager
    if ($packageManager -notmatch '^pnpm@[0-9]+\.[0-9]+\.[0-9]+$') {
        throw "Launcher package.json must declare an exact pnpm packageManager version."
    }
    if (-not (Test-Path (Join-Path $launcherRepoDir "node_modules"))) {
        Write-Host "Installing launcher dependencies..." -ForegroundColor Yellow
        & npx --yes $packageManager install --frozen-lockfile
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to install launcher dependencies in $launcherRepoDir"
        }
    }
    $env:NEOOMSI_PATH = $engineExe
    & npx --yes $packageManager exec vitest run src/tests/process-client.test.ts -t "real neoomsi engine process" --reporter=default --reporter=json --outputFile="$reportFile"
    $vitestExitCode = $LASTEXITCODE
    if ($vitestExitCode -ne 0) {
        throw "Launcher TypeScript ProcessEngineClient live integration test failed with exit code $vitestExitCode"
    }
    if (-not (Test-Path $reportFile)) {
        throw "Vitest did not produce expected report at $reportFile"
    }
    $report = Get-Content $reportFile -Raw | ConvertFrom-Json
    if ($report.numPassedTests -lt 1) {
        throw "Expected at least 1 test to pass in the live engine integration test, but report indicated $($report.numPassedTests) passed and $($report.numPendingTests) skipped."
    }
    if ($report.numFailedTests -gt 0) {
        throw "Vitest report indicated $($report.numFailedTests) failed test(s)."
    }
    Write-Host "Launcher TypeScript ProcessEngineClient successfully communicated with engine ($($report.numPassedTests) passed)." -ForegroundColor Green
} finally {
    Remove-Item Env:\NEOOMSI_PATH -ErrorAction SilentlyContinue
    Remove-Item $reportFile -Force -ErrorAction SilentlyContinue
    Pop-Location
}

# 7. Verify explicit error when launcher is missing (no silent native fallback)
Write-Host "`nStep 7: Verifying missing launcher error behavior..." -ForegroundColor Yellow
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
try {
    New-Item -ItemType Directory -Force $tempDir | Out-Null
    $isolatedEngine = Join-Path $tempDir "neoomsi.exe"
    Copy-Item $engineExe $isolatedEngine
    Get-ChildItem -Path $distWin -Filter "*.dll" -ErrorAction SilentlyContinue | ForEach-Object {
        Copy-Item $_.FullName $tempDir
    }

    function Run-ProcessWithTimeout([string]$filePath, [int]$timeoutMs = 10000) {
        $pinfo = New-Object System.Diagnostics.ProcessStartInfo
        $pinfo.FileName = $filePath
        $pinfo.RedirectStandardOutput = $true
        $pinfo.RedirectStandardError = $true
        $pinfo.UseShellExecute = $false
        $proc = [System.Diagnostics.Process]::Start($pinfo)

        $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
        $stderrTask = $proc.StandardError.ReadToEndAsync()

        $exited = $proc.WaitForExit($timeoutMs)
        if (-not $exited) {
            try { $proc.Kill() } catch {}
            $proc.WaitForExit(2000)
            throw "Process '$filePath' timed out after $timeoutMs ms and was terminated."
        }

        # Wait for stream reading tasks to complete
        [System.Threading.Tasks.Task]::WaitAll(@($stdoutTask, $stderrTask), 2000)
        $stdout = if ($stdoutTask.IsCompleted) { $stdoutTask.Result } else { "" }
        $stderr = if ($stderrTask.IsCompleted) { $stderrTask.Result } else { "" }

        return [PSCustomObject]@{
            ExitCode = $proc.ExitCode
            Output   = "$stdout`n$stderr"
        }
    }

    # Test bare launch
    $res1 = Run-ProcessWithTimeout $isolatedEngine 10000
    if ($res1.ExitCode -eq 0) {
        throw "Expected neoomsi.exe to fail when launcher is missing, but exited with 0"
    }
    if ($res1.Output -notmatch "desktop launcher was not found") {
        throw "Expected missing launcher error message, but got:`n$($res1.Output)"
    }
    Write-Host "  neoomsi.exe correctly failed when launcher was absent (no silent fallback)." -ForegroundColor Green

    # Test neoomsi-launcher.exe without launcher
    $isolatedLauncher = Join-Path $tempDir "neoomsi-launcher.exe"
    Copy-Item (Join-Path $distWin "neoomsi-launcher.exe") $isolatedLauncher
    $res2 = Run-ProcessWithTimeout $isolatedLauncher 10000
    if ($res2.ExitCode -eq 0) {
        throw "Expected neoomsi-launcher.exe to fail when launcher is missing, but exited with 0"
    }
    if ($res2.Output -notmatch "desktop launcher was not found") {
        throw "Expected missing launcher error message from neoomsi-launcher.exe, but got:`n$($res2.Output)"
    }
    Write-Host "  neoomsi-launcher.exe correctly failed when launcher was absent." -ForegroundColor Green
} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Recurse -Force $tempDir
    }
}

Write-Host "`n=== Combined Bundle Validation Passed Successfully! ===" -ForegroundColor Green
