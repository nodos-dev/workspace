@echo off
setlocal

:: Configuration
set PROJECT_DIR=Toolchain\nosman
set OUTPUT_NAME=nodos.exe
set BIN_NAME=nosman.exe

:: Check if dir exists
if not exist "%PROJECT_DIR%" (
    echo.
    echo [ERROR] Could not find directory "%PROJECT_DIR%"
    exit /b 1
)

pushd %PROJECT_DIR%

echo Building nosman
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERROR] Build failed
    popd
    exit /b %ERRORLEVEL%
)

popd
echo Copying "%PROJECT_DIR%\target\release\%BIN_NAME%" to ".\%OUTPUT_NAME%"
move /y "%PROJECT_DIR%\target\release\%BIN_NAME%" ".\%OUTPUT_NAME%"

echo.
echo Success