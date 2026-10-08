@echo off
REM Rover Doctor - checks what is wrong, in plain language.
REM Run this when the Rust dashboard does not work. It checks:
REM   1. Rust toolchain exists
REM   2. Rust backend reachable on port 8000 - /api/health
REM   3. MQTT broker reachable on port 1883
REM   4. Dashboard pages answer
REM   5. ESP32 rover heartbeat - online or offline
title Rover Doctor
cd /d "%~dp0"

echo ================= Rover Doctor =================
echo.

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
for /f "tokens=*" %%v in ('cargo --version 2^>^&1') do echo [OK] %%v
goto backend

:nocargo
echo [X] Rust toolchain: NOT FOUND - install from https://rustup.rs/
goto backend

:backend
echo.
echo --- Rust backend on port 8000 ---
curl -s -m 5 http://localhost:8000/api/health
echo.
echo.
echo If empty or an error shows above: backend is NOT running. Double-click Rover.bat first.
echo If mqtt_connected is false: broker is down - Rover.bat starts it, or open the Setup page.
echo If device_online is false: ESP32 is not publishing - check rover power and WiFi.
goto mqtt

:mqtt
echo.
echo --- MQTT broker on port 1883 ---
netstat -an | findstr /c:":1883" | findstr /i "listening"
if errorlevel 1 goto nomqtt
echo [OK] Something is listening on 1883 - see the line above.
goto pages

:nomqtt
echo [X] Nothing listening on port 1883.
echo Rover.bat starts Mosquitto for you - run it. Service install needs admin.
goto pages

:pages
echo.
echo --- Dashboard pages, expect 200 each ---
curl -s -o nul -w "this pc:       %%{http_code}\n" -m 5 http://localhost:8000/
curl -s -o nul -w "setup page:    %%{http_code}\n" -m 5 http://localhost:8000/setup/
curl -s -o nul -w "calibration:   %%{http_code}\n" -m 5 http://localhost:8000/calibration/
curl -s -o nul -w "detailed:      %%{http_code}\n" -m 5 http://localhost:8000/detailed/
curl -s -o nul -w "test page:     %%{http_code}\n" -m 5 http://localhost:8000/test/
goto heartbeat

:heartbeat
echo.
echo --- ESP32 heartbeat ---
curl -s -m 5 http://localhost:8000/api/device/status
echo.
echo.
echo online true means the rover is publishing. online false means check rover power and WiFi.
echo.
echo ================= Done =================
pause
