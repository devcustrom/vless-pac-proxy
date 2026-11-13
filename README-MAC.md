# macOS: установка и особенности

Бинарник `wireproxy-control` — универсальный (Intel + Apple Silicon). Код общий
с Windows-версией; отличия — движки под архитектуру мака, системный прокси
через `networksetup`, автозапуск через launchd и иконка в строке меню.

> macOS-часть собирается в GitHub Actions и проверена сборкой, но не прошла
> ручной смоук на живом Mac. Считайте её **beta**: если что-то не работает,
> запустите `./wireproxy-control --no-tray` и смотрите `manager.log`.

## Установка

1. Скачайте `wireproxy-control-macos-universal.zip` из Releases и распакуйте.
2. Снимите карантин (Gatekeeper блокирует неподписанные бинарники):

   ```bash
   xattr -dr com.apple.quarantine wireproxy-control
   chmod +x wireproxy-control
   ```

   Либо: ПКМ → «Открыть» → «Открыть» в Finder.

3. Запустите:

   ```bash
   ./wireproxy-control
   ```

   При первом запуске скачаются `wireproxy` и `xray` (проверка sha256),
   создастся папка данных, откроется панель в браузере.

4. В панели: добавьте `vless://`-ссылку или положите `wg.conf` рядом с
   бинарником, затем включите системный прокси.

Данные: рядом с бинарником, если там есть `wg.conf`/`settings.json`
(portable), иначе `~/Library/Application Support/wireproxy-control`.
Переопределить: `--data-dir ~/wireproxy`.

## Автозапуск

```bash
./wireproxy-control install
```

Создаёт `~/Library/LaunchAgents/com.wireproxy-control.plist` с `KeepAlive` и
скрытым запуском. Включить сразу:

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.wireproxy-control.plist
```

Выключить:

```bash
./wireproxy-control uninstall
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.wireproxy-control.plist
```

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

Меню: «Открыть панель», «Установить сертификаты…», «Обновить списки доменов»,
«Перезапустить движки», «Выход» (с выключением системного прокси и остановкой
движков).

«Открыть панель» ведёт на **https://panel.localhost/**, если домен
`panel.localhost` привязан к панели и CA уже в доверенных; иначе —
`http://127.0.0.1:8080/`.

## Локальные домены и HTTPS

1. Блок «Локальные домены»: `app.localhost = 3000`.
2. HTTPS: кнопка «Установить сертификаты» запускает
   `local-domains-trust.command` (запросит пароль администратора):

   ```bash
   sudo security add-trusted-cert -d -r trustRoot \
     -k /Library/Keychains/System.keychain certs/ca.crt
   ```

3. `*.localhost` резолвится браузером без hosts. Для настоящих имён:

   ```bash
   ./wireproxy-control sync-hosts
   ./local-domains-hosts.command   # sudo внутри
   ```

4. Firefox: `security.enterprise_roots.enabled = true` в `about:config`.

Порты 80/443 заданы в `settings.json` (`localProxyPort`, `localHttpsPort`).
Если macOS не даёт занять их без прав, поставьте 8088/8443 — ошибка видна в
панели, а домены продолжат работать по этим портам.

## Команды

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
