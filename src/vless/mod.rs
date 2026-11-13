//! VLESS/SS: парсер ссылок, пул подписок, авто-ротация.
pub mod link;
pub mod pool;

/// Локальный HTTP-порт xray, через который идёт VLESS-канал.
pub const XRAY_HTTP_PORT: u16 = 25346;
/// SOCKS5-порт wireproxy.
pub const WIREPROXY_SOCKS_PORT: u16 = 25344;
/// HTTP-порт wireproxy (upstream умного прокси для WG).
pub const WIREPROXY_HTTP_PORT: u16 = 25345;
