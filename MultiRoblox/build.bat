@echo off
setlocal
title Build
cd /d "%~dp0" || (echo ERROR: Could not enter the project directory & pause & exit /b 1)
echo Building MultiRoblox executable...
echo.

where cargo >nul 2>&1
if errorlevel 1 (echo Rust toolchain not found - install from https://rustup.rs & pause & exit /b 1)

set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (echo ERROR: .NET Framework C# compiler not found & pause & exit /b 1)

rem Validate the version before using it in a directory deletion.
set "BUILD_VERSION="
for /f "delims=" %%V in ('powershell.exe -NoProfile -Command "$ErrorActionPreference = 'Stop'; $v = (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).version; if ($v -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Expected a major.minor.patch app version' }; Write-Output $v"') do set "BUILD_VERSION=%%V"
if not defined BUILD_VERSION (echo ERROR: Could not read a valid app version & pause & exit /b 1)
set "BUILD_OUTPUT=dist\release_%BUILD_VERSION%"

if exist "%BUILD_OUTPUT%" (
  echo Removing previous output: %BUILD_OUTPUT%
  rmdir /s /q "%BUILD_OUTPUT%"
  if errorlevel 1 (echo ERROR: Could not remove previous output - close any running release executable & pause & exit /b 1)
)

echo Precompiling native helper...
"%CSC%" /nologo /optimize+ /platform:x64 /target:exe /r:System.Drawing.dll /out:src-tauri\resources\RobloxNative.exe src-tauri\resources\RobloxNative.cs
if errorlevel 1 (echo ERROR: Native helper build failed - main build stopped & pause & exit /b 1)
if not exist "src-tauri\resources\RobloxNative.exe" (echo ERROR: Native helper output missing - main build stopped & pause & exit /b 1)

cargo build --release --manifest-path src-tauri\Cargo.toml --bin MultiRoblox
if errorlevel 1 (echo Build failed & pause & exit /b 1)

mkdir "%BUILD_OUTPUT%"
if errorlevel 1 (echo ERROR: Could not create release directory & pause & exit /b 1)
copy /y "src-tauri\target\release\MultiRoblox.exe" "%BUILD_OUTPUT%\MultiRoblox.exe" >nul
if errorlevel 1 (echo Could not copy MultiRoblox.exe & pause & exit /b 1)

echo.
echo Done. Executable: %BUILD_OUTPUT%\MultiRoblox.exe
pause
exit /b 0
