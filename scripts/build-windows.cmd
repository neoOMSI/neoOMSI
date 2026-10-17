@echo off
rem Build neoOMSI for Windows into dist\windows.
rem By default this compiles the engine binary.
rem Usage: scripts\build-windows.cmd [target]
setlocal
cd /d "%~dp0\.."
set "TARGET=%~1"
if "%TARGET%"=="" set "TARGET=x86_64-pc-windows-msvc"

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust from https://rustup.rs using the MSVC toolchain, then run this script again.
  exit /b 1
)
cargo build --locked --release --target %TARGET% -p core
if errorlevel 1 goto :failed
if not exist "dist\windows" mkdir "dist\windows"
copy /y "target\%TARGET%\release\neoomsi.exe" "dist\windows\neoomsi.exe" >nul || goto :failed

echo.
echo Done. Run: "%CD%\dist\windows\neoomsi.exe"
exit /b 0
:failed
echo.
echo Build failed. See the error above.
exit /b 1
