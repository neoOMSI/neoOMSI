@echo off
rem Build neoOMSI for Windows into dist\windows (neoomsi.exe is the game and, started with
rem no arguments, the launcher window): x64, or ARM64 with the target as the first argument
rem (build-windows.cmd aarch64-pc-windows-msvc). Needs Rust with the MSVC toolchain
rem (https://rustup.rs) and Visual Studio Build Tools with "Desktop development with C++"
rem (for ARM64 also its ARM64 build tools and LLVM's clang) and the Windows SDK.
rem To build the Windows version on a Mac, use scripts/build-windows-cross.sh instead.
setlocal
cd /d "%~dp0\.."
set "TARGET=%~1"
if "%TARGET%"=="" set "TARGET=x86_64-pc-windows-msvc"
set "LAUNCHER_ARCH=x64"
if "%TARGET%"=="aarch64-pc-windows-msvc" set "LAUNCHER_ARCH=arm64"
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
call scripts\build-windows-launcher.cmd "dist\windows\launcher" "%LAUNCHER_ARCH%"
if errorlevel 1 goto :failed
echo.
echo Done. Run: "%CD%\dist\windows\neoomsi.exe"
exit /b 0
:failed
echo.
echo Build failed. See the error above.
exit /b 1
