@echo off
echo ========================================
echo Building VelocityDB Server (Debug Mode)
echo ========================================
echo.

cargo build

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo Starting Server with Verbose Logging
    echo ========================================
    echo.
    
    set RUST_LOG=debug
    cargo run -- server --bind 127.0.0.1:2005 --data-dir ./velocitydb --verbose
) else (
    echo.
    echo Build failed!
    pause
)
