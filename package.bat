@echo off
REM RoverDash packager - assembles portable dist\RoverDash\ from this checkout.
REM Build first (slow): trunk build inside frontend-wasm\ for the Rust UI
REM bundle (plain dev build ON PURPOSE: release LTO emits bulk-memory ops
REM that the pinned wasm-opt rejects, and the dev bundle is what all
REM eyeball tests ran against), then cargo build --release -p roverdash.
REM The folder is self-contained: exe + Rust UI (dist\next) + pages + broker.
REM Data (config, history, baselines) is created beside the exe on first run.
setlocal
cd /d "%~dp0"
if not exist "target\release\roverdash.exe" (
  echo [ERROR] target\release\roverdash.exe missing - run cargo build --release -p roverdash first.
  exit /b 1
)
if not exist "dist\next\index.html" (
  echo [ERROR] dist\next\index.html missing - run trunk build in frontend-wasm\ first.
  exit /b 1
)
set "DIST=%~dp0dist\RoverDash"
rmdir /s /q "%DIST%" 2>nul
mkdir "%DIST%" || exit /b 1
copy /y "target\release\roverdash.exe" "%DIST%\" || exit /b 1
xcopy "web" "%DIST%\web\" /e /i /q || exit /b 1
xcopy "dist\next" "%DIST%\dist\next\" /e /i /q || exit /b 1
copy /y "desktop\binaries\mosquitto-x86_64-pc-windows-msvc.exe" "%DIST%\" || exit /b 1
copy /y "desktop\binaries\mosquitto.dll" "%DIST%\" || exit /b 1
copy /y "desktop\binaries\pthreadVC3.dll" "%DIST%\" || exit /b 1
echo [OK] Portable app ready: %DIST%
echo      Double-click roverdash.exe. First run creates rover_config.json + history\ beside it.
