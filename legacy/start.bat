@echo off
cd /d %~dp0
rem Отладочный запуск: менеджер в видимом окне (логи вживую) + иконка в трее.
start "wireproxy-manager" cmd /c node manager.js
start "wireproxy-tray" powershell -NoProfile -ExecutionPolicy Bypass -File tray.ps1
echo Manager started (visible window) + tray icon.
echo   Panel:  http://127.0.0.1:8080/
echo   PAC:    http://127.0.0.1:8080/proxy.pac
