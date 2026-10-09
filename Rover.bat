@echo off
REM Rover Digital Twin 2.0 - double-click launcher
REM What it does for you, in order:
REM   1. Checks the Rust toolchain is installed
REM   2. Starts the MQTT broker if nothing listens on 1883
REM   3. Starts the Rust backend, which serves API + dashboard on ONE port: 8000
REM Open the dashboard on THIS pc: http://localhost:8000/
REM (localhost-only by design - no LAN mode, see README.)
REM Full transcript of every run goes to Rover.log next to this file.
title Rover Digital Twin 2.0
cd /d "%~dp0"
set "LOG=%~dp0Rover.log"
set "PORT=8000"

REM Single-instance fast path: backend already answering means another
REM launcher owns it - just open the dashboard, never start a second copy.
curl.exe -s -m 3 http://localhost:8000/api/health >nul 2>&1
if not errorlevel 1 goto alreadyrunning

echo ---------------------------------------- >> "%LOG%"
echo Rover start >> "%LOG%"

REM Find a cargo that REALLY runs. Probe by execution, never by path alone.
cargo --version >nul 2>&1
if not errorlevel 1 goto havecargo
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" goto tryusercargo
goto nocargo

:tryusercargo
"%USERPROFILE%\.cargo\bin\cargo.exe" --version >nul 2>&1
if errorlevel 1 goto nocargo
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
goto havecargo

:havecargo
echo [Rover] Rust toolchain found. >> "%LOG%" 2>&1
cargo --version >> "%LOG%" 2>&1
goto checkmqtt

:nocargo
echo.
echo [ERROR] Rust toolchain not found.
echo Install Rust stable for Windows from https://rustup.rs/
echo Then close this window, open a new one, and run Rover again.
echo [nocargo] no working cargo >> "%LOG%" 2>&1
echo.
pause
goto end

:checkmqtt
netstat -an | findstr /c:":1883" | findstr /i "listening" >nul
if not errorlevel 1 goto havemqtt
goto trymqtt

:havemqtt
echo [Rover] MQTT broker already listening on 1883. >> "%LOG%" 2>&1
goto startserver

:trymqtt
if exist "C:\Program Files\mosquitto\mosquitto.exe" goto startmqtt
goto nomqtt

:startmqtt
echo [Rover] No broker on 1883 - starting Mosquitto in the background...
echo [startmqtt] launching userland mosquitto >> "%LOG%" 2>&1
start "" /min "C:\Program Files\mosquitto\mosquitto.exe" -v >> "%LOG%" 2>&1
goto startserver

:nomqtt
echo.
echo [WARNING] Nothing listening on 1883 and Mosquitto was not found.
echo The dashboard will open but show NOT connected to broker.
echo Install Mosquitto on this PC, then run Rover again.
echo [nomqtt] no broker, continuing anyway >> "%LOG%" 2>&1
echo.
goto startserver

:startserver
echo [Rover] Starting Rust backend on port 8000...
echo [Rover] This PC:  http://localhost:8000/
echo [Rover] Setup page for the broker address:  http://localhost:8000/setup/
echo [Rover] Browser opens automatically once the backend answers...
start "" /min cmd /c ""%~dp0bat\open_when_ready.bat""
REM The app logs here too - if another launcher holds this file, starting a
REM second copy would die silently, so check before running.
type nul >> "%LOG%" 2>nul
if errorlevel 1 goto loglocked
cargo run >> "%LOG%" 2>&1
goto stopped

:loglocked
curl.exe -s -m 3 http://localhost:8000/api/health >nul 2>&1
if not errorlevel 1 goto alreadyrunning
echo [WARNING] Rover.log is locked by another program - showing output in this window instead.
cargo run
goto stopped

:stopped
echo.
echo [Rover] Backend stopped. Press any key to close this window.
pause >nul
goto end

:alreadyrunning
echo.
echo [Rover] Backend already running - opening the dashboard instead of a second copy...
start http://localhost:8000/
goto end

:end
