#!/bin/bash
# Установка wireproxy-стека на macOS:
#   бинарник под архитектуру + LaunchAgent (автозапуск при входе, перезапуск при падении)
# Требуется Node.js (brew install node).
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$DIR"

# 1. Node.js
if ! command -v node >/dev/null 2>&1; then
  echo "Node.js не найден. Установи: brew install node   (или с https://nodejs.org)"
  exit 1
fi

# 2. Бинарник wireproxy под архитектуру мака
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) ASSET="wireproxy_darwin_arm64.tar.gz" ;;
  *)     ASSET="wireproxy_darwin_amd64.tar.gz" ;;
esac
if [ ! -x ./wireproxy ]; then
  echo "Скачиваю $ASSET ..."
  curl -sL -o wp.tar.gz "https://github.com/windtf/wireproxy/releases/download/v1.1.3/$ASSET"
  tar -xzf wp.tar.gz
  rm -f wp.tar.gz
  chmod +x ./wireproxy
fi
# снять карантин, если бинарник скачан браузером
xattr -d com.apple.quarantine ./wireproxy 2>/dev/null || true

# 2b. Xray-core (для VLESS-канала)
XASSET="Xray-macos-64.zip"
[ "$ARCH" = "arm64" ] && XASSET="Xray-macos-arm64-v8a.zip"
if [ ! -x ./xray ]; then
  echo "Скачиваю $XASSET ..."
  curl -sL -o xray.zip "https://github.com/XTLS/Xray-core/releases/download/v26.10.21/$XASSET"
  unzip -o -q xray.zip xray
  rm -f xray.zip
  chmod +x ./xray
fi
xattr -d com.apple.quarantine ./xray 2>/dev/null || true

# 3. Конфиги
if [ ! -f ./wg.conf ]; then
  echo ""
  echo "ВНИМАНИЕ: нет wg.conf — канал WireGuard будет отключён."
  echo "Стек всё равно запустится; можно работать только через VLESS —"
  echo "вставьте vless:// ссылку в панели. Либо положите сюда конфиг"
  echo "ОТДЕЛЬНОГО клиента (не копируйте ключ с других устройств!)."
  echo ""
fi
if [ ! -f ./wireproxy.conf ]; then
  cat > wireproxy.conf <<EOF
WGConfig = $DIR/wg.conf

[Socks5]
BindAddress = 127.0.0.1:25344

[http]
BindAddress = 127.0.0.1:25345
EOF
else
  # файл скопировали с Windows? чиним путь до wg.conf
  sed -i '' "s|^WGConfig = .*|WGConfig = $DIR/wg.conf|" wireproxy.conf
fi

# 4. LaunchAgent: автозапуск + KeepAlive
mkdir -p "$HOME/Library/LaunchAgents"
NODE_BIN="$(command -v node)"
sed -e "s|__DIR__|$DIR|g" -e "s|__NODE__|$NODE_BIN|g" \
  "$DIR/com.wireproxy.manager.plist.template" \
  > "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist"
launchctl unload "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist" 2>/dev/null || true
launchctl load "$HOME/Library/LaunchAgents/com.wireproxy.manager.plist"

sleep 4
echo ""
echo "Проверка:"
curl -s -m 5 -o /dev/null -w "  панель: http://127.0.0.1:8080/ -> HTTP %{http_code}\n" http://127.0.0.1:8080/ || echo "  панель не отвечает, смотри manager.log"
echo ""
echo "Готово. Дальше:"
echo "  - панель:        http://127.0.0.1:8080/"
echo "  - системный прокси: галка в панели (применится к активному Wi-Fi/Ethernet)"
echo "  - автозапуск:    launchd, стартует при входе, сам перезапускается"
echo "  - логи:          $DIR/manager.log"
