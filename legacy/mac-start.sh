#!/bin/bash
# Запуск стека на macOS (обычно не нужен — LaunchAgent стартует сам при входе)
launchctl load "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist" 2>/dev/null && echo "менеджер запущен" \
  || echo "LaunchAgent не найден — сначала запусти ./mac-install.sh"
[ -f "$HOME/Library/LaunchAgents/com.wireproxy.tray.plist" ] && launchctl load "$HOME/Library/LaunchAgents/com.wireproxy.tray.plist" 2>/dev/null && echo "иконка меню-бара запущена"
