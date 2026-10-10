@echo off
rem Build the pinned Electron launcher into the Windows package folder passed as %1.
rem This keeps local packages on the same launcher UI as nightly builds.
setlocal
cd /d "%~dp0\.."
set "LAUNCHER_DEST=%~1"
set "LAUNCHER_ARCH=%~2"
if "%LAUNCHER_DEST%"=="" (
  echo Usage: scripts\build-windows-launcher.cmd ^<package-launcher-folder^> [x64^|arm64]
  exit /b 1
)
if "%LAUNCHER_ARCH%"=="" set "LAUNCHER_ARCH=x64"

set "BASH=%ProgramFiles%\Git\bin\bash.exe"
if not exist "%BASH%" set "BASH=%ProgramFiles(x86)%\Git\bin\bash.exe"
if not exist "%BASH%" (
  echo Git Bash is required to build the Electron launcher. Install Git for Windows, then run this script again.
  exit /b 1
)

if not exist "dist\windows" mkdir "dist\windows"
"%BASH%" "%CD%/scripts/ci/build-launcher.sh" windows %LAUNCHER_ARCH%
if errorlevel 1 exit /b %ERRORLEVEL%

if /I "%LAUNCHER_DEST%"=="dist\windows\launcher" exit /b 0
if exist "%LAUNCHER_DEST%" rmdir /s /q "%LAUNCHER_DEST%"
robocopy "dist\windows\launcher" "%LAUNCHER_DEST%" /e /njh /njs /ndl /nfl >nul
if errorlevel 8 exit /b %ERRORLEVEL%
exit /b 0
