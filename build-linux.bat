@echo off
echo ========================================
echo VelocityDB - Linux Build
echo ========================================
echo.
echo Building for Linux using WSL...
echo.

REM Build in the current directory (WSL will use the Windows path automatically)
wsl bash -c "source $HOME/.cargo/env && cargo build --release"

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo SUCCESS!
    echo ========================================
    echo.
    echo Linux binary created at:
    echo target\release\velocity
    echo.
    echo File info:
    wsl bash -c "ls -lh target/release/velocity"
    echo.
    echo You can now copy this file to your Linux machine.
    echo To run on Linux: 
    echo   chmod +x velocity
    echo   ./velocity
    echo.
) else (
    echo.
    echo ========================================
    echo BUILD FAILED
    echo ========================================
    echo.
    echo Possible issues:
    echo - Rust not installed in WSL
    echo - Compilation errors
    echo.
    echo To install Rust in WSL, run:
    echo wsl bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo.
)

pause
