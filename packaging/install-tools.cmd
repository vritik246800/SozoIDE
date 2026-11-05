@echo off
rem Installs the tools that make SozoIDE better on Windows, with winget, after asking.
rem Safe to run again: what is there stays. --check only says what is missing, --yes does
rem not ask. tmux has no Windows build: inside WSL, run install-tools.sh.
setlocal enabledelayedexpansion
set "check="
set "yes="
for %%a in (%*) do (
    if /i "%%a"=="--check" set "check=1"
    if /i "%%a"=="--yes" set "yes=1"
)
set "missing="
call :look git Git.Git
call :look curl cURL.cURL
call :look ffmpeg Gyan.FFmpeg
if not defined missing exit /b 0
if defined check exit /b 1
where winget >nul 2>nul
if errorlevel 1 (
    echo winget is missing: install App Installer from the Microsoft Store.
    exit /b 1
)
if not defined yes (
    set /p "answer=Install with winget? [y/N] "
    if /i not "!answer:~0,1!"=="y" (
        echo Nothing installed. SozoIDE works without them.
        exit /b 0
    )
)
for %%i in (%missing%) do (
    winget install --id %%i --exact --silent --accept-package-agreements --accept-source-agreements --disable-interactivity
    if errorlevel 1 set "failed=1"
)
if defined failed exit /b 1
echo Open a new terminal for the PATH to change.
exit /b 0

:look
where %1 >nul 2>nul
if errorlevel 1 (
    echo Missing  %1
    set "missing=!missing! %2"
) else (
    echo OK       %1
)
exit /b 0
