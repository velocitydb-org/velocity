@echo off
echo ========================================
echo Starting VelocityDB Server...
echo ========================================
echo.
echo Server will listen on: 127.0.0.1:2005
echo Data directory: ./velocitydb
echo.
echo Press Ctrl+C to stop the server
echo.
echo ========================================
echo.

cargo run --release -- server --bind 127.0.0.1:2005 --data-dir ./velocitydb
