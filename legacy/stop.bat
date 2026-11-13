@echo off
rem Сначала просим менеджер выключить системный прокси (пока он жив)
curl -s -m 3 -X POST http://127.0.0.1:8080/api/system-proxy -H "Content-Type: application/json" -H "X-WPC: 1" -d "{""enabled"":false}" >nul 2>&1
rem Страховка, если менеджер уже не отвечает: снимаем настройку прокси напрямую в реестре
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings" /v ProxyEnable /t REG_DWORD /d 0 /f >nul 2>&1
rem Убираем env-прокси, чтобы приложения с собственным HTTP-клиентом
rem (opencode и т.п.) после остановки стека ходили напрямую, а не в мёртвый порт
powershell -NoProfile -Command "[Environment]::SetEnvironmentVariable('HTTPS_PROXY',$null,'User'); [Environment]::SetEnvironmentVariable('HTTP_PROXY',$null,'User'); [Environment]::SetEnvironmentVariable('NODE_USE_ENV_PROXY',$null,'User')" >nul 2>&1
taskkill /F /IM wireproxy.exe >nul 2>&1
taskkill /F /IM xray.exe >nul 2>&1
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and (($_.Name -eq 'node.exe' -and $_.CommandLine -match 'manager\.js') -or ($_.Name -eq 'powershell.exe' -and $_.CommandLine -match 'tray\.ps1')) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }" >nul 2>&1
echo wireproxy, xray, manager and tray stopped; system proxy and env vars disabled.
