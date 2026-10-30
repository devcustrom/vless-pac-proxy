#!/bin/bash
# Остановка стека на macOS (manager + wireproxy + иконка меню-бара)
# Сначала штатно выключаем системный прокси через менеджер (пока он жив)
curl -s -m 3 -X POST http://127.0.0.1:8080/api/system-proxy -H 'Content-Type: application/json' -d '{"enabled":false}' >/dev/null 2>&1 || true
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist" 2>/dev/null || true
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.tray.plist" 2>/dev/null || true
pkill -f "WireproxyTray" 2>/dev/null || true
pkill -f "wireproxy -c wireproxy.conf" 2>/dev/null || true
pkill -f "xray -c xray-config.json" 2>/dev/null || true
echo "wireproxy-стек остановлен. Запуск заново: ./mac-start.sh  или просто перелогинься."
