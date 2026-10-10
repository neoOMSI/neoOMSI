@echo off
rem Build neoOMSI for local release-style testing into dist\windows-dev. It is quicker than
rem the proper release build, but is not the version to use for a PR or published release.
setlocal
cd /d "%~dp0\.."
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust from https://rustup.rs using the MSVC toolchain, then run this script again.
  exit /b 1
)
cargo build --locked --profile dev-release -p core -p legacy-launcher-core
if errorlevel 1 goto :failed
if not exist "dist\windows-dev" mkdir "dist\windows-dev"
copy /y "target\dev-release\neoomsi.exe" "dist\windows-dev\neoomsi.exe" >nul || goto :failed
copy /y "target\dev-release\neoomsi-launcher.exe" "dist\windows-dev\neoomsi-launcher.exe" >nul || goto :failed
call scripts\build-windows-launcher.cmd "dist\windows-dev\launcher"
if errorlevel 1 goto :failed
echo.
echo Done. Run: "%CD%\dist\windows-dev\neoomsi.exe"
exit /b 0
:failed
echo.
echo Build failed. See the error above.
exit /b 1
