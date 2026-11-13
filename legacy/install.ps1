# Установка стека на Windows: скачивает wireproxy.exe и xray.exe, создаёт конфиги из примеров.
# Запуск: powershell -ExecutionPolicy Bypass -File install.ps1 [-Autostart]
param([switch]$Autostart)
$ErrorActionPreference = 'Stop'
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $dir

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  Write-Error "Node.js не найден. Установите с https://nodejs.org/ и повторите."
}

if (-not (Test-Path .\wireproxy.exe)) {
  Write-Output "Скачиваю wireproxy v1.1.3 ..."
  Invoke-WebRequest 'https://github.com/windtf/wireproxy/releases/download/v1.1.3/wireproxy_windows_amd64.tar.gz' -OutFile wp.tar.gz
  tar -xzf wp.tar.gz
  Remove-Item wp.tar.gz
}

if (-not (Test-Path .\xray.exe)) {
  Write-Output "Скачиваю Xray-core v26.10.21 ..."
  Invoke-WebRequest 'https://github.com/XTLS/Xray-core/releases/download/v26.10.21/Xray-windows-64.zip' -OutFile xray.zip
  Expand-Archive xray.zip -DestinationPath . -Force
  Remove-Item xray.zip
}

if (-not (Test-Path .\wg.conf)) {
  Copy-Item wg.conf.example wg.conf
  Write-Warning "wg.conf создан из примера — впишите РЕАЛЬНЫЙ конфиг вашего туннеля (Приватный ключ нельзя коммитить)!"
}
if (-not (Test-Path .\wireproxy.conf)) { Copy-Item wireproxy.conf.example wireproxy.conf }

if ($Autostart) {
  $ws = New-Object -ComObject WScript.Shell
  $startup = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup"
  $lnk = $ws.CreateShortcut("$startup\wireproxy.lnk")
  $lnk.TargetPath = "$env:SystemRoot\System32\wscript.exe"
  $lnk.Arguments = "`"$dir\start-hidden.vbs`""
  $lnk.WorkingDirectory = $dir
  $lnk.Save()
  Write-Output "Автозапуск: ярлык wireproxy.lnk создан в Автозагрузке"
}

Write-Output ""
Write-Output "Готово. Запуск: start-hidden.vbs (скрыто) или start.bat (с окнами, для отладки)."
Write-Output "Панель управления: http://127.0.0.1:8080/"
