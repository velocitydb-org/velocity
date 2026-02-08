@echo off
echo ========================================
echo Building VelocityDB Server...
echo ========================================
echo.

cargo build --release

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo Build successful!
    echo ========================================
    echo.
    echo Now run the server with:
    echo cargo run --release -- server --bind 127.0.0.1:2005 --data-dir ./velocitydb
    echo.
) else (
    echo.
    echo ========================================
    echo Build failed! Check errors above.
    echo ========================================
    echo.
)

pause
