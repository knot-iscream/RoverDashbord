@echo off
REM Rover-dev - fast NATIVE iteration loop (debug, Rust live view).
REM Runs the desktop shell from source: main dashboard window + second
REM "Rover Rust View" window pushed by Rust with real telemetry (~2Hz).
REM NOTE: closing the window only hides it to the tray (single instance).
REM To relaunch fresh: Quit from the tray menu (or kill roverdash.exe),
REM then restart this script and confirm "Compiling roverdash" appears -
REM config/capability changes compile into the binary and NEVER apply
REM to an already-running process. web/ edits need only Ctrl+R (Ctrl+F5
REM after shared CSS/JS changes); src/* or desktop/* edits need a restart.
title Rover-dev (native debug loop)
cd /d "%~dp0"

cargo --version >nul 2>&1
if not errorlevel 1 goto havedev
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cargo --version >nul 2>&1
if errorlevel 1 goto nocargo

:havedev
echo [Rover-dev] Debug build + native windows (main + Rust live view)...
echo [Rover-dev] Real rover data only - no simulation. Close windows or Ctrl+C to stop.
cargo run -p roverdash
goto end

:nocargo
echo.
echo [ERROR] Rust toolchain not found - install from https://rustup.rs/
echo.
pause

:end
