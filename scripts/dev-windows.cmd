@echo off
rem Build and run neoOMSI for local testing. This uses Cargo's development profile and packages
rem the same Electron launcher UI as nightly builds. Pass game arguments after the script name if needed.
setlocal
cd /d "%~dp0\.."
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust from https://rustup.rs using the MSVC toolchain, then run this script again.
  exit /b 1
)
cargo build --locked -p core -p legacy-launcher-core
if errorlevel 1 goto :failed
if not exist "dist\windows-debug" mkdir "dist\windows-debug"
copy /y "target\debug\neoomsi.exe" "dist\windows-debug\neoomsi.exe" >nul || goto :failed
copy /y "target\debug\neoomsi-launcher.exe" "dist\windows-debug\neoomsi-launcher.exe" >nul || goto :failed
call scripts\build-windows-launcher.cmd "dist\windows-debug\launcher"
if errorlevel 1 goto :failed
"dist\windows-debug\neoomsi.exe" %*
exit /b %ERRORLEVEL%
:failed
echo.
echo Build failed. See the error above.
exit /b 1
