# tg-ws-proxy sidecar (vendored)

Это минимальная headless-копия ядра
[Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy) `v1.11.4`,
собранная как sidecar для `wireproxy-control`.

- Upstream license: MIT, copyright 2026 Flowseal — см. `LICENSE`.
- Исходники `proxy/` сохранены с upstream; изменения:
  - `proxy/upstream.py` — исходящие пути direct / WireGuard / VLESS через
    локальные HTTP CONNECT-прокси;
  - `proxy/tg_ws_proxy.py` — managed-config, status-файл, редактирование
    секрета в логах, отключение авто-загрузки CF-списка в managed-режиме;
  - `proxy/raw_websocket.py`, `proxy/bridge.py` — использование opener’а;
  - `proxy/health.py` — фоновые проверки WS/TCP-доступности DC и сброс
    backoff пула при восстановлении маршрута.
- Tray/GUI/i18n upstream не включены.

## Сборка

Windows:

```powershell
.\build.ps1
```

macOS/Linux:

```bash
./build.sh
```

Нужен Python 3.11+ (используется `StreamWriter.start_tls` для CONNECT+TLS).
Результат: `dist/tgws-proxy.exe` (Windows) или `dist/tgws-proxy`.

Для локальной проверки положите бинарник в каталог данных
`wireproxy-control` под именем `tgws-proxy.exe` / `tgws-proxy`.
