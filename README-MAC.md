# macOS: установка и особенности

Бинарник `wireproxy-control` — универсальный (Intel + Apple Silicon). Код общий
с Windows-версией; отличия — движки под архитектуру мака, системный прокси
через `networksetup`, автозапуск через launchd и иконка в строке меню.

> macOS-часть собирается в GitHub Actions и проверена сборкой, но не прошла
> ручной смоук на живом Mac. Считайте её **beta**: если что-то не работает,
> запустите `./wireproxy-control --no-tray` и смотрите `manager.log`.

## Установка

1. Скачайте из Releases один из файлов:
   - `wireproxy-control-macos-universal.dmg` — откройте образ и перетащите
     `WireproxyControl.app` в «Программы»;
   - `wireproxy-control-macos-universal.zip` — распакуйте и перенесите
     `WireproxyControl.app` в «Программы»;
   - `wireproxy-control-macos-universal` — обычный бинарник для запуска из
     Терминала (для продвинутых).

2. Первый запуск: ПКМ по `WireproxyControl.app` → «Открыть» → «Открыть».
   macOS предупредит о неподписанном приложении — это ожидаемо, сборка
   подписана ad-hoc. Если система пишет «повреждено», выполните в Терминале:

   ```bash
   xattr -dr com.apple.quarantine /Applications/WireproxyControl.app
   open /Applications/WireproxyControl.app
   ```

3. Приложение скачает `wireproxy` и `xray` (проверка sha256), создаст папку
   данных и откроет панель в браузере. Дальше всё делается в панели: добавьте
   `vless://`-ссылку или импортируйте `wg.conf`, включите системный прокси.
   Для локальных доменов можно сразу включить плагин и выбрать домен панели.

Данные: `~/Library/Application Support/wireproxy-control` (или рядом с
бинарником, если запускать его из папки с `wg.conf`/`settings.json`).
Переопределить: `--data-dir ~/wireproxy`.

## Автозапуск

Для `.app` проще всего добавить `WireproxyControl.app` в «Системные настройки»
→ «Основные» → «Элементы входа». Альтернатива — launchd:

```bash
/Applications/WireproxyControl.app/Contents/MacOS/wireproxy-control install
```

Создаёт `~/Library/LaunchAgents/com.wireproxy-control.plist` с `KeepAlive` и
скрытым запуском. Включить сразу:

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.wireproxy-control.plist
```

Выключить:

```bash
/Applications/WireproxyControl.app/Contents/MacOS/wireproxy-control uninstall
```

Плитка удаляется; текущий процесс продолжает работать, а со следующего входа
агент больше не запускается.

Логи launchd и менеджера: `manager.log` в папке данных.

## Системный прокси

Галка «Системный прокси» в панели (или иконка в меню-баре → трей) проходит по
всем активным сетевым сервисам из `networksetup -listallnetworkservices` и
выставляет web/secure web proxy на `127.0.0.1:8081` плюс список исключений
(localhost, локальные подсети, `*.local`).

- При выходе прокси выключается автоматически — браузер не останется без
  интернета.
- Переключились Wi-Fi ↔ Ethernet с другим набором сервисов — просто снимите и
  поставьте галку заново.
- Вручную: Системные настройки → Сеть → активный сервис → Подробнее → Прокси.

## Иконка в строке меню

Меню: статус каналов, «Системный прокси», «Telegram-прокси»,
«Открыть Telegram», «Открыть панель», «Настройки…», «Выход» (с выключением
системного прокси и остановкой движков).

«Открыть панель» ведёт на **https://<домен панели>/**, если плагин локальных
доменов включён, выбранный домен привязан к панели и CA уже в доверенных;
иначе — `http://127.0.0.1:8080/`. Если плагин выключен, трей всегда открывает
loopback-адрес.

## Локальные домены и HTTPS

1. Блок «Локальные домены»: `app.localhost = 3000`.
2. HTTPS: кнопка «Установить сертификаты» показывает нативный запрос пароля
   администратора (без окна Терминала) и добавляет CA в системный keychain:

   ```bash
   sudo security add-trusted-cert -d -r trustRoot \
     -k /Library/Keychains/System.keychain \
     "$HOME/Library/Application Support/wireproxy-control/certs/ca.crt"
   ```

3. `*.localhost` резолвится браузером без hosts. Для настоящих имён нажмите
   «Применить hosts» — скрипт выполнится с правами администратора, снова без
   окна Терминала. Эквивалент из командной строки:

   ```bash
   ./wireproxy-control sync-hosts
   ./local-domains-hosts.command
   ```

4. Firefox: `security.enterprise_roots.enabled = true` в `about:config`.

На macOS порты 80/443 требуют root, поэтому при первом включении плагина
приложение автоматически занимает свободные 8088/8443 и сохраняет их в
`settings.json` (`localProxyPort`, `localHttpsPort`). Ссылки в панели уже
содержат нужный порт (`http://app.localhost:8088`,
`https://app.localhost:8443`). Если хотите вернуть 80/443, запускайте менеджер
от root — но это не рекомендуется.

## Команды

Для `.app` бинарник лежит внутри:
`/Applications/WireproxyControl.app/Contents/MacOS/wireproxy-control`.
В таблице он сокращён до `wireproxy-control`.

| Действие | Команда |
| --- | --- |
| Запуск (трей) | `./wireproxy-control` |
| Без трея | `./wireproxy-control --no-tray` |
| Остановить | `./wireproxy-control stop` |
| Автозапуск вкл/выкл | `./wireproxy-control install` / `uninstall` |
| Сертификаты | `./wireproxy-control trust-certs` |
| hosts | `./wireproxy-control sync-hosts` |
| Логи | `tail -f ~/Library/Application\ Support/wireproxy-control/manager.log` |
| Статус API | `curl -s http://127.0.0.1:8080/api/status` |

## Проверка работы

```bash
curl -s http://127.0.0.1:8080/api/status | python3 -m json.tool | head -40
curl -x http://127.0.0.1:8081 https://api.ipify.org; echo
```

Второй IP должен совпадать с выходным адресом VPN, а не с адресом провайдера.
Точечная проверка домена — карточка «Проверка сайта» в панели.

## Отличия от Windows

- Иконка в строке меню, а не в системном трее.
- Автозапуск — LaunchAgent с `KeepAlive`; после падения launchd поднимет сам.
- Доверие CA — системный keychain (`sudo`), а не `certutil`.
- Системный прокси — `networksetup`, список сервисов определяется
  автоматически.
