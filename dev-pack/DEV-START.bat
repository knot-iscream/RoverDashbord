@echo off
cd /d "%~dp0"
echo RoverDash DEV launcher
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0dev-start.ps1"
echo.
pause
