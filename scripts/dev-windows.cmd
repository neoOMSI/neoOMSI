@echo off
rem Build and run neoOMSI for local testing. This uses Cargo's development profile, so it is
rem quicker than a release build. Pass game arguments after the script name if needed.
setlocal
cd /d "%~dp0\.."
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Install Rust from https://rustup.rs using the MSVC toolchain, then run this script again.
  exit /b 1
)
cargo run --locked -p omsi-app --bin neoomsi -- %*
