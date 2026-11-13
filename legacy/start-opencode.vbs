' Запуск OpenCode desktop с прокси ТОЛЬКО для этого приложения.
' Глобальные переменные окружения не нужны: env задаётся лишь для процесса
' этого скрипта и наследуется запущенным OpenCode.
' Ярлык на этот файл можно положить на рабочий стол/панель задач.
Option Explicit
Dim sh, env, exe
Set sh = CreateObject("WScript.Shell")
Set env = sh.Environment("Process")
env("HTTPS_PROXY") = "http://127.0.0.1:8081"
env("HTTP_PROXY") = "http://127.0.0.1:8081"
env("NO_PROXY") = "localhost,127.0.0.1,::1,.local,.z.ai"
env("NODE_USE_ENV_PROXY") = "1"
exe = sh.ExpandEnvironmentStrings("%LOCALAPPDATA%") & "\Programs\ai.opencode.desktop\OpenCode.exe"
sh.Run """" & exe & """", 1, False
