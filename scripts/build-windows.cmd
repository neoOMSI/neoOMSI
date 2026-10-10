@echo off
rem Build neoOMSI for Windows into dist\windows.
rem By default this compiles the engine and legacy-launcher-core binaries.
rem Pass --package-launcher to also bundle the pinned Electron launcher.
rem Usage: scripts\build-windows.cmd [target] [--package-launcher]
setlocal
cd /d "%~dp0\.."
set "PACKAGE_LAUNCHER=0"
if /I "%~1"=="--package-launcher" (
  set "TARGET=x86_64-pc-windows-msvc"
  set "PACKAGE_LAUNCHER=1"
) else (
  set "TARGET=%~1"
  if /I "%~2"=="--package-launcher" set "PACKAGE_LAUNCHER=1"
)
if "%TARGET%"=="" set "TARGET=x86_64-pc-windows-msvc"

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust from https://rustup.rs using the MSVC toolchain, then run this script again.
  exit /b 1
)
cargo build --locked --release --target %TARGET% -p core -p legacy-launcher-core
if errorlevel 1 goto :failed
if not exist "dist\windows" mkdir "dist\windows"
copy /y "target\%TARGET%\release\neoomsi.exe" "dist\windows\neoomsi.exe" >nul || goto :failed
copy /y "target\%TARGET%\release\neoomsi-launcher.exe" "dist\windows\neoomsi-launcher.exe" >nul || goto :failed

if not "%PACKAGE_LAUNCHER%"=="1" goto :done_launcher
set "LAUNCHER_ARCH=x64"
if "%TARGET%"=="aarch64-pc-windows-msvc" set "LAUNCHER_ARCH=arm64"
set "BASH=%ProgramFiles%\Git\bin\bash.exe"
if not exist "%BASH%" set "BASH=%ProgramFiles(x86)%\Git\bin\bash.exe"
if not exist "%BASH%" (
  for /f "delims=" %%i in ('where bash 2^>nul') do (
    set "BASH=%%i"
    goto :bash_found
  )
)
:bash_found
if not exist "%BASH%" (
  echo Git Bash is required to package the Electron launcher.
  goto :failed
)
"%BASH%" "%CD%/scripts/ci/build-launcher.sh" windows %LAUNCHER_ARCH%
if errorlevel 1 goto :failed
:done_launcher

echo.
echo Done. Run: "%CD%\dist\windows\neoomsi.exe"
exit /b 0
:failed
echo.
echo Build failed. See the error above.
exit /b 1
