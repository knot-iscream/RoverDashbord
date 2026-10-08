@echo off
REM Rover helper - waits until the Rust backend answers, then opens the dashboard.
REM Started minimized by Rover.bat. Backend port is 8000 by design.
title Rover opener
set "LOG=%~dp0..\Rover-opener.log"
set /a N=0

:wait
curl.exe -s -m 3 http://localhost:8000/api/health >nul 2>&1
if errorlevel 1 goto notyet
echo [opener] backend answered, opening browser >> "%LOG%" 2>&1
start http://localhost:8000/
goto end

:notyet
set /a N=%N%+1
if %N% GEQ 30 goto giveup
timeout /t 2 /nobreak >nul
goto wait

:giveup
echo [Rover] Backend did not answer within 60 seconds. Open Rover.log for details.
echo [opener] backend never answered >> "%LOG%" 2>&1
pause
goto end

:end
exit
