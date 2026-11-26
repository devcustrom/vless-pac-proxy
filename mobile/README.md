# Wireproxy Mobile

Standalone Android-клиент: VPN на всё устройство, Telegram-only MTProto/WSS и
no-root обход DPI. Это отдельный проект, не пульт для desktop-версии.

## Текущий статус

Готово:

- `mobile/rust` — общий Rust-core:
  - парсинг `vless://`, `vmess://`, `trojan://`, `ss://`, Outline;
  - разбор WireGuard `.conf` и подписок;
  - генерация Xray-конфига с SOCKS/TUN, direct, DNS и fragment/ByeDPI;
  - 26 unit-тестов, `cargo fmt` и `clippy -D warnings`.
- `mobile/android` — Kotlin/Compose приложение:
  - режимы VPN и Telegram, обход DPI и стратегии ByeDPI;
  - импорт ссылок, WireGuard и подписок;
  - профили и подписки в зашифрованном Keystore-хранилище;
  - автопоиск быстрейшего профиля через `pingBatch`;
  - VPN: `VpnService` + `hev-socks5-tunnel` + Xray SOCKS;
  - DNS через DoH внутри туннеля, private CIDR и direct-домены;
  - Telegram-only: локальный MTProto/WSS-прокси на `127.0.0.1:1443`,
    CF-домены, TCP fallback и `tg://proxy`-ссылка;
  - ByeDPI (`ciadpi`) для Telegram upstream; direct-домены в VPN идут
    через Xray с TLS-фрагментацией;
  - split tunneling: список приложений вне VPN;
  - Always-on VPN / kill switch через системные настройки;
  - unit-, instrumentation- и smoke-тесты TUN/Telegram/ByeDPI.

- автообновление APK из GitHub Releases: проверка раз в 12 часов и вручную
  во вкладке «Настройки», загрузка с проверкой SHA-256 и установка через
  Android Package Installer.

Не входит в эту итерацию:

- автоматическая установка без подтверждения пользователя (ограничение Android);
- iOS;
- root-режим с `nfqws`.

## Сборка

Требуется Android SDK 35, JDK 17+ и Gradle 8.9 (wrapper уже в репозитории).

```bash
# libXray AAR: libxray-android.zip из XTLS/libXray releases
curl -L -o /tmp/libxray.zip \
  https://github.com/XTLS/libXray/releases/download/v26.11.9/libxray-android.zip
unzip -j /tmp/libxray.zip libxray-android/libXray.aar \
  -d mobile/android/app/libs/

cd mobile/android
./gradlew :app:assembleDebug
```

APK: `app/build/outputs/apk/debug/app-debug.apk`.

Release APK подписывается, если заданы переменные окружения:

```bash
export WIREPROXY_ANDROID_KEYSTORE=/path/to/release.keystore
export WIREPROXY_ANDROID_KEYSTORE_PASSWORD=...
export WIREPROXY_ANDROID_KEY_ALIAS=...
export WIREPROXY_ANDROID_KEY_PASSWORD=...
./gradlew :app:assembleRelease
```

Без них собирается `app-release-unsigned.apk`.

Rust-core:

```bash
cd mobile/rust
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Архитектура

```text
Compose UI
  ├─ VPN: VpnService TUN
  │    → hev-socks5-tunnel
  │      → Xray SOCKS inbound
  │        → WireGuard/VLESS/VMess/Trojan/SS/Outline
  │        → direct + TLS fragment для direct-доменов
  │
  └─ Telegram: foreground service
       → локальный MTProto/WSS-прокси
         → WebSocket / Telegram DC / ByeDPI upstream
```

Xray-core поставляется `libXray` (MPL-2.0), TUN-мост — `hev-socks5-tunnel`
(MIT), обход DPI — `ciadpi`/ByeDPI (MIT), UI — Jetpack Compose (Apache-2.0).
Пересобрать ByeDPI: `ANDROID_NDK_HOME=/path/to/ndk ./mobile/scripts/build-ciadpi.sh`.

## Проверки

Локально на эмуляторе/устройстве:

```bash
./gradlew :app:testDebugUnitTest :app:lintDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk

# Крипто MTProto и сплиттер пакетов
adb shell am instrument -w \
  -e class com.devcustrom.wireproxy.telegram.TelegramProxyInstrumentedTest \
  com.devcustrom.wireproxy.test/androidx.test.runner.AndroidJUnitRunner

# Реальный запрос req_pq_multi через локальный прокси к DC Telegram
# (тот же тест выше)

# ByeDPI: запуск ciadpi и HTTPS через SOCKS
adb shell am instrument -w \
  -e class com.devcustrom.wireproxy.core.ByeDpiInstrumentedTest \
  com.devcustrom.wireproxy.test/androidx.test.runner.AndroidJUnitRunner

# Подписки: fetch → libXray → encrypted store → замена профилей
adb shell am instrument -w \
  -e class com.devcustrom.wireproxy.core.SubscriptionInstrumentedTest \
  com.devcustrom.wireproxy.test/androidx.test.runner.AndroidJUnitRunner
```

Для smoke-теста VPN (debug-сборка):

```bash
adb shell am start -n com.devcustrom.wireproxy/.debug.DebugVpnActivity \
  --es wgFile /data/user/0/com.devcustrom.wireproxy/files/wg.conf \
  --ez dpi true --es directDomains example.com
# затем проверить TCP/HTTP из shell (трафик shell идёт через tun0)
```

## CI

`.github/workflows/android.yml`:

- Rust: format, clippy, tests.
- Android: unit tests, lint, debug APK artifact.

Release-подпись включается переменными окружения
`WIREPROXY_ANDROID_KEYSTORE`, `WIREPROXY_ANDROID_KEYSTORE_PASSWORD`,
`WIREPROXY_ANDROID_KEY_ALIAS`, `WIREPROXY_ANDROID_KEY_PASSWORD`.

## Автообновление APK

APK публикуется в GitHub Releases под именем
`wireproxy-control-android.apk` вместе с `.apk.sha256`. Сборка запускается
по push тега `v*`; тег должен совпадать с версией в `Cargo.toml` и
`web/package.json`. Приложение:

- раз в 12 часов проверяет релизы GitHub (WorkManager);
- показывает уведомление при новой версии;
- проверяет SHA-256 и имя пакета перед установкой;
- открывает системный установщик Android;
- версия APK в CI берётся из тега (`-PappVersionName`), versionCode
  вычисляется из `major*100_000_000 + minor*1_000_000 + patch`
  (например, `0.6.13` → `6000013`); допустимо `major<=20`, `minor<=99`,
  `patch<=999999`.

Первый APK с автообновлением устанавливается вручную. Дальше приложение само
увидит следующий релиз с `wireproxy-control-android.apk`: покажет уведомление,
скачает APK, сверит SHA-256 и версию, после чего откроет системный установщик.
Если установленный APK подписан debug-ключом, обновиться до release-сборки
нельзя — Android потребует удалить приложение.

Для подписи релиза создайте постоянный ключ и задайте GitHub Secrets:

```bash
keytool -genkeypair -v \
  -keystore wireproxy-release.keystore \
  -alias wireproxy -keyalg RSA -keysize 4096 -validity 10000 \
  -storepass '<пароль>' -keypass '<пароль>' \
  -dname "CN=Wireproxy, O=Wireproxy, C=RU"
base64 -w0 wireproxy-release.keystore
```

В PowerShell (Windows):

```powershell
[Convert]::ToBase64String(
  [IO.File]::ReadAllBytes("$HOME\.wireproxy-android\release.keystore")
)
```

Секреты добавляются в GitHub: **Settings → Secrets and variables → Actions →
New repository secret**.

- `ANDROID_KEYSTORE_BASE64` — вывод `base64 -w0` (Linux/macOS) или
  `ToBase64String` (Windows) одной строкой;
- `ANDROID_KEYSTORE_PASSWORD`;
- `ANDROID_KEY_ALIAS` (`wireproxy`);
- `ANDROID_KEY_PASSWORD`.

Keystore и файл с паролем храните **вне репозитория**: `*.keystore`,
`*.jks` и `keystore.properties` добавлены в `.gitignore`.

Ключ нужно сохранить: все будущие APK должны быть подписаны им же,
иначе Android откажется обновлять приложение.

Без этих секретов Android-задача release workflow пропускается, а desktop/
sidecar-релизы продолжают собираться. Если Android-сборка упала, релиз
desktop/sidecar всё равно публикуется, но workflow останется красным.
Первый установленный APK и все последующие обновления должны быть подписаны
**одним и тем же** ключом, иначе Android откажется устанавливать обновление.
