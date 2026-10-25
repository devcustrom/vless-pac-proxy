#!/bin/bash
# Остановка стека на macOS (manager + wireproxy)
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist" 2>/dev/null || true
pkill -f "wireproxy -c wireproxy.conf" 2>/dev/null || true
echo "wireproxy-стек остановлен. Запуск заново: ./mac-start.sh  или просто перелогинься."
