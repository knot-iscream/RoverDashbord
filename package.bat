@echo off
REM RoverDash packager - assembles portable dist\RoverDash\ from this checkout.
REM Run `cargo build --release -p roverdash` first (slow: LTO + Tauri).
REM The folder is self-contained: exe + pages + broker. Data (config, history,
REM baselines) is created beside the exe on first run.
setlocal
cd /d "%~dp0"
if not exist "target\release\roverdash.exe" (
  echo [ERROR] target\release\roverdash.exe missing - run cargo build --release -p roverdash first.
  exit /b 1
)
set "DIST=%~dp0dist\RoverDash"
rmdir /s /q "%DIST%" 2>nul
mkdir "%DIST%" || exit /b 1
copy /y "target\release\roverdash.exe" "%DIST%\" || exit /b 1
xcopy "web" "%DIST%\web\" /e /i /q || exit /b 1
copy /y "desktop\binaries\mosquitto-x86_64-pc-windows-msvc.exe" "%DIST%\" || exit /b 1
copy /y "desktop\binaries\mosquitto.dll" "%DIST%\" || exit /b 1
copy /y "desktop\binaries\pthreadVC3.dll" "%DIST%\" || exit /b 1
echo [OK] Portable app ready: %DIST%
echo      Double-click roverdash.exe. First run creates rover_config.json + history\ beside it.
