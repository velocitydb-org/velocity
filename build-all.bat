@echo off
setlocal
set TARGET_LINUX=x86_64-unknown-linux-gnu
set DIST_DIR=dist

echo Building Windows release...
cargo build --release
if errorlevel 1 goto fail

echo Cross-compiling Linux release (%TARGET_LINUX%)...
cross build --target %TARGET_LINUX% --release
if errorlevel 1 goto fail

if not exist %DIST_DIR% mkdir %DIST_DIR%
copy /Y target\release\velocity %DIST_DIR%\velocity-windows.exe > nul
copy /Y target\%TARGET_LINUX%\release\velocity %DIST_DIR%\velocity-linux-x64 > nul

echo Build artifacts available under %DIST_DIR%
exit /b 0

:fail
echo Build failed with exit code %errorlevel%
exit /b %errorlevel%
