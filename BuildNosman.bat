@echo off
setlocal

:: Configuration
set PROJECT_DIR=Toolchain\nosman
set OUTPUT_NAME=nodos.exe
set BIN_NAME=nosman.exe

pushd %PROJECT_DIR%

echo Building
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERROR] Build failed. Please check your Rust code.
    popd
    exit /b %ERRORLEVEL%
)

popd
move /y "%PROJECT_DIR%\target\release\%BIN_NAME%" ".\%OUTPUT_NAME%"

echo.
echo Success