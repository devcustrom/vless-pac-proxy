' Скрытый запуск всего стека (без единого окна):
'   1) manager.js — демон: управляет wireproxy, раздаёт PAC и веб-панель
'   2) tray.ps1   — иконка в системном трее
'   3) env-переменные прокси для приложений со своим HTTP-клиентом
'      (opencode, VS Code и т.п.) — stop.bat их убирает.
' Ярлык в Автозагрузке должен указывать на этот файл.
Option Explicit
Dim fso, sh, baseDir
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")
baseDir = fso.GetParentFolderName(WScript.ScriptFullName)
sh.CurrentDirectory = baseDir
sh.Run "cmd /c node manager.js > manager.log 2>&1", 0, False
sh.Run "powershell -NoProfile -ExecutionPolicy Bypass -File """ & baseDir & "\tray.ps1""", 0, False
sh.Run "powershell -NoProfile -Command ""[Environment]::SetEnvironmentVariable('HTTPS_PROXY','http://127.0.0.1:8081','User'); [Environment]::SetEnvironmentVariable('HTTP_PROXY','http://127.0.0.1:8081','User'); [Environment]::SetEnvironmentVariable('NO_PROXY','localhost,127.0.0.1,::1,.local,.z.ai','User'); [Environment]::SetEnvironmentVariable('NODE_USE_ENV_PROXY','1','User')""", 0, False
