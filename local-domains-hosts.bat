@echo off
rem Adds domains from local-domains.txt to the hosts file (needs Administrator).
net session >nul 2>&1
if errorlevel 1 (
  echo Please run this file as Administrator ^(right click - Run as administrator^).
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0hosts-sync.ps1"
pause
