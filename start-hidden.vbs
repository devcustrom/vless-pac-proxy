' Скрытый запуск всего стека (без единого окна):
'   1) manager.js — демон: управляет wireproxy, раздаёт PAC и веб-панель
'   2) tray.ps1   — иконка в системном трее
' Ярлык в Автозагрузке должен указывать на этот файл.
Option Explicit
Dim fso, sh, baseDir
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")
baseDir = fso.GetParentFolderName(WScript.ScriptFullName)
sh.CurrentDirectory = baseDir
sh.Run "cmd /c node manager.js > manager.log 2>&1", 0, False
sh.Run "powershell -NoProfile -ExecutionPolicy Bypass -File """ & baseDir & "\tray.ps1""", 0, False
