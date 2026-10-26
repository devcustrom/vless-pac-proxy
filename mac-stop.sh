#!/bin/bash
# Остановка стека на macOS (manager + wireproxy + иконка меню-бара)
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist" 2>/dev/null || true
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.tray.plist" 2>/dev/null || true
pkill -f "WireproxyTray" 2>/dev/null || true
pkill -f "wireproxy -c wireproxy.conf" 2>/dev/null || true
pkill -f "xray -c xray-config.json" 2>/dev/null || true
echo "wireproxy-стек остановлен. Запуск заново: ./mac-start.sh  или просто перелогинься."
