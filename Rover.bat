@echo off
REM RoverDash launcher - starts the app (native window, own backend+broker).
REM Single-instance is enforced by the app itself: launching twice just
REM focuses the running window, so starting is always safe.
title RoverDash
cd /d "%~dp0"
set "APP=%~dp0dist\RoverDash\roverdash.exe"
set "LOG=%~dp0Rover.log"

echo ---------------------------------------- >> "%LOG%"
echo RoverDash start >> "%LOG%"

if exist "%APP%" goto haveapp
goto devfallback

:haveapp
echo [Rover] Starting RoverDash...
start "" "%APP%" >> "%LOG%" 2>&1
goto end

:devfallback
REM No packaged build (dist\) - run the backend from source instead.
REM Needs the Rust toolchain; the dashboard opens in your browser.
cargo --version >nul 2>&1
if not errorlevel 1 goto havedev
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" goto tryusercargo
goto nocargo

:tryusercargo
"%USERPROFILE%\.cargo\bin\cargo.exe" --version >nul 2>&1
if errorlevel 1 goto nocargo
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
goto havedev

:havedev
netstat -an | findstr /c:":1883" | findstr /i "listening" >nul
if not errorlevel 1 goto devrun
if exist "C:\Program Files\mosquitto\mosquitto.exe" start "" /min "C:\Program Files\mosquitto\mosquitto.exe" -v >> "%LOG%" 2>&1

:devrun
echo [Rover] Dev mode: backend from source, dashboard at http://localhost:8000/
echo [Rover] (Run package.bat once for the native window.)
start "" /min cmd /c "cargo run >> "%LOG%" 2>&1"
set /a TRIES=0

:waitup
curl.exe -s -m 2 http://localhost:8000/api/health >nul 2>&1
if not errorlevel 1 goto openit
set /a TRIES+=1
if %TRIES% GEQ 30 goto openanyway
timeout /t 2 /nobreak >nul
goto waitup

:openanyway
echo [Rover] Backend is slow to answer - opening anyway, hit refresh if needed.

:openit
start http://localhost:8000/
goto end

:nocargo
echo.
echo [ERROR] No packaged app (dist\RoverDash missing) and no Rust toolchain.
echo Either run package.bat on a dev PC, or install Rust from https://rustup.rs/
echo.
pause
goto end

:end
