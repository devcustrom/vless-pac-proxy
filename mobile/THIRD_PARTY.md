# Third-party components

- **libXray / Xray-core** — MPL-2.0, <https://github.com/XTLS/libXray>,
  <https://github.com/XTLS/Xray-core>. The AAR is downloaded at build time and
  is not stored in this repository.
- **hev-socks5-tunnel** — MIT, <https://github.com/heiher/hev-socks5-tunnel>.
  The prebuilt AAR in `android/app/libs/` is built from upstream sources.
- **ByeDPI / ciadpi** — MIT, <https://github.com/hufrea/byedpi>. The binaries
  in `android/app/src/main/jniLibs/` are built from upstream sources with the
  Android NDK.
- **OkHttp** — Apache-2.0, <https://github.com/square/okhttp>.
- **AndroidX / Jetpack Compose** — Apache-2.0,
  <https://developer.android.com/jetpack>.
- **Cloudflare-fronted Telegram WS domain list** — derived from
  <https://github.com/Flowseal/tg-ws-proxy> (MIT).

The application talks to Telegram DC endpoints and Cloudflare-fronted WS
gateways. It is not affiliated with Telegram or Cloudflare.
