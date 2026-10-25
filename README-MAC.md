# Тот же стек на macOS

Ядро (manager.js, update-domains.js, ui.html) кроссплатформенное — один код
работает и на Windows, и на маке. Отличия: бинарник wireproxy, системный
прокси через networksetup и автозапуск через launchd вместо трея.

## Шаги

1. **Скопируй папку стека** `C:\Users\devcu\wireproxy` на мак (например,
   `~/wireproxy`). Нужны: `manager.js`, `update-domains.js`, `ui.html`,
   `proxy.pac`, `extra-domains.txt`, `mac-*.sh`, `com.wireproxy.manager.plist.template`.
   `wireproxy.exe`, `*.bat`, `*.vbs`, `tray.ps1` на маке не нужны.
   Внимание: `wg.conf` из папки НЕ используй (см. шаг 2).

2. **Создай ОТДЕЛЬНЫЙ peer для мака** в панели WireGuard-сервера и его конфиг
   положи как `~/wireproxy/wg.conf`.
   Нельзя запускать два устройства с одним ключом одновременно — сессии
   перебивают друг друга (сервер видит один peer, endpoint прыгает).

3. **Установи Node.js** (если нет): `brew install node`.

4. **Запусти установщик:**
   ```bash
   cd ~/wireproxy
   chmod +x mac-*.sh
   ./mac-install.sh
   ```
   Скрипт скачает wireproxy под архитектуру (Apple Silicon/Intel), пропишет
   LaunchAgent (автозапуск при входе + перезапуск при падении) и проверит панель.

5. **Включи системный прокси** галкой в панели http://127.0.0.1:8080/ —
   применится к активному сетевому сервису (Wi-Fi или Ethernet, определяется
   автоматически по default-маршруту). На macOS web+secure proxy + bypass
   домены выставляются через networksetup.

6. Перезапусти браузер — и пользуйся. Логика маршрутов та же: списки РКН и
   extra-domains через VPN (твой NL-сервер), остальное напрямую. Не забудь,
   что «Недавние сайты (мимо VPN)» и «Проверка сайта» в панели работают так же.

## Повседневные команды

- стоп:  `./mac-stop.sh`
- старт: `./mac-start.sh` (или просто перелогинься — LaunchAgent сам поднимет)
- логи:  `tail -f ~/wireproxy/manager.log`
- удалить автозапуск: `launchctl unload ~/Library/LaunchAgents/com.wireproxy.manager.plist && rm ~/Library/LaunchAgents/com.wireproxy.manager.plist`

## Отличия от Windows

- Нет иконки в трее (WinForms-трей непереносим). Управление — панель в
  браузере; при желании позже можно сделать Swift status-bar иконку.
- Системный прокси — через `networksetup`, применяется к активному сервису;
  если переключаешься между Wi-Fi и Ethernet — просто сними/поставь галку заново.
- `macOS Gatekeeper`: если заблокирует бинарник — Системные настройки ->
  Конфиденциальность и безопасность -> «Всё равно открыть» (скрипт уже
  пробует снять карантин через xattr).
- Remote mobile (z.ai) в bypass-списке — учтено в кроссплатформенном коде.

## Если что-то не так

- Панель не поднимается: `cat manager.log`
- Проверить, что wireproxy жив: `pgrep -fl wireproxy`
- Проверить маршрут: `curl -x http://127.0.0.1:8081 https://claude.com/cdn-cgi/trace`
  (должен быть ip NL-сервера)
- Два устройства с одним ключом «воюют» — см. шаг 2.
