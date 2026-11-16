# Плагины и расширение

`wireproxy-control` — один бинарник, но внутренние функции разделены на плагины.
Плагин — это модуль бэкенда + флаг в `settings.json` + карточка/вкладка в панели.
Выключенный плагин не запускает свои слушатели и не влияет на трафик.

## Существующие плагины

| Плагин | Что делает | Флаг в settings.json |
| --- | --- | --- |
| Локальные домены | HTTP :80 / HTTPS :443, SNI, WebSocket, свой CA | `localDomainsEnabled` (по умолчанию `true`) |
| Умный прокси | маршрутизация по доменам на :8081 | всегда включён (ядро) |
| Списки доменов | itdog + Re-filter RKN, PAC | всегда включён (ядро) |
| Пул VLESS/VMess/Trojan/SS | подписки, live-проверка, ротация | всегда включён (ядро) |

Отключить плагин можно в панели: вкладка **«Домены»** → переключатель
«Плагин локальных доменов», либо через API:

```bash
curl -X POST -H "X-WPC: 1" -H "Content-Type: application/json" \
  -d '{"localDomains": false}' http://127.0.0.1:8080/api/plugins
```

Текущее состояние плагинов есть в `/api/status` → `plugins`.

## Как устроен плагин локальных доменов

1. `src/net/local_proxy.rs` — HTTP/HTTPS-слушатели, разбор `Host`/SNI,
   реверс-прокси на loopback, WebSocket.
2. `src/local_tls.rs` — локальный CA и сертификаты (rcgen).
3. `src/local_hosts.rs` — `hosts-entries.txt` и скрипты синхронизации hosts.
4. `AppState.local_domains`, `local_http_error`, `local_https_error`,
   `tls_config`, `local_proxy_stop`.
5. API: `/api/local-domains`, `/probe`, `/hosts`, `/api/plugins`.
6. UI: `web/src/components/LocalDomainsCard.vue`, вкладка «Домены».

Запуск/остановка:

```rust
crate::net::local_proxy::start(&state); // идемпотентно
crate::net::local_proxy::stop(&state);  // закрывает слушатели, освобождает порты
```

При включении плагин добавляет домены в исключения системного прокси,
чтобы браузер не отправлял их в :8081.

## Как добавить новый плагин

1. **Модуль.** Создайте `src/my_plugin.rs`, публичные `start(&Arc<AppState>)`
   и `stop(&Arc<AppState>)`. Долгоживущие задачи запускайте через `tokio::spawn`
   и храните `watch`/`oneshot` канал остановки в `AppState`.
2. **Настройки.** Добавьте поле в `src/settings.rs`:

   ```rust
   #[serde(rename = "myPluginEnabled", default = "default_true")]
   pub my_plugin_enabled: bool,
   ```

   `Settings::save` уже пишет JSON атомарно; старые `settings.json`
   подхватятся благодаря `default`.
3. **Состояние.** При необходимости добавьте поля в `AppState` и инициализацию
   в `AppState::new`.
4. **API.** В `src/api/mod.rs` добавьте маршруты. Все POST требуют заголовок
   `X-WPC: 1`; guard уже проверяет Host и токен. Отдавайте JSON, ошибки —
   `StatusCode::BAD_REQUEST` + `{"error": "..."}`.
5. **Плагины.** В `/api/status` добавьте `plugins.myPlugin`.
   В `/api/plugins` обработайте `myPlugin` так же, как `localDomains`:
   сохраните настройку, вызовите `start`/`stop`.
6. **UI.** Создайте карточку в `web/src/components/`, подключите её во вкладке
   `App.vue`, читайте состояние из `store.status.plugins`.
7. **Безопасность.** Внешние запросы — только http/https, проверяйте host через
   `net::dns_guard` (loopback/private/reserved запрещены). Не логируйте ключи и
   пароли. Данные — только через `paths::in_dir` внутри каталога данных.
8. **Тесты.** Добавьте юнит-тесты парсеров/валидаторов в модуль; сетевые
   проверки — вручную или в отдельном интеграционном сценарии.

## Как добавить VPN-протокол

Протоколы живут в `src/vless/link.rs`:

1. Добавьте `parse_<proto>` (валидация полей, `ParsedLink`), ветку в `parse_any`.
2. В `build_outbound` соберите outbound для Xray (или другого движка).
3. Разрешите схему в `parse_subscription_links` (`src/vless/pool.rs`).
4. Добавьте тест: парсинг + `build_outbound` содержит нужный `protocol`.
5. UI-подписи (`VLESS / SS`) обновите в `VlessCard.vue` и README.

Уже поддержаны: `vless://` (Reality/TLS/ws/grpc), `vmess://` (base64 JSON),
`trojan://`, `ss://` (SIP002 и legacy). Новые протоколы автоматически
попадают в общий пул, live-проверку и автопереключение.

## Несколько WireGuard-конфигов

Файлы лежат в `wg-configs/*.conf`, активный — `wg.conf` (копия выбранного).
API: `GET /api/wg/configs`, `POST /api/wg/configs` с действиями
`add | use | remove | test`. Перед применением тест поднимает временный
wireproxy на :26249/:26250 и проверяет реальный трафик. Тест и выбор — в
карточке WireGuard (вкладка «Конфиги»).

## Отладка

- `manager.log` в каталоге данных — запуск движков, ошибки, переходы каналов.
- `/api/status` — полное состояние (каналы, плагины, пул, списки).
- `npm run dev` в `web/` — панель на :5173 с проксированием `/api` на :8080.
