@echo off
rem Сначала просим менеджер выключить системный прокси (пока он жив)
curl -s -m 3 -X POST http://127.0.0.1:8080/api/system-proxy -H "Content-Type: application/json" -d "{""enabled"":false}" >nul 2>&1
rem Страховка, если менеджер уже не отвечает: снимаем настройку прокси напрямую в реестре
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings" /v ProxyEnable /t REG_DWORD /d 0 /f >nul 2>&1
taskkill /F /IM wireproxy.exe >nul 2>&1
taskkill /F /IM xray.exe >nul 2>&1
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and (($_.Name -eq 'node.exe' -and $_.CommandLine -match 'manager\.js') -or ($_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'tray\.ps1')) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }" >nul 2>&1
echo wireproxy, xray, manager and tray stopped; system proxy disabled.
