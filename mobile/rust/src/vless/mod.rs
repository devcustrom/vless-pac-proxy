/// Same loopback port as desktop; mobile uses it only for the local Xray
/// HTTP/SOCKS bridge.
pub const XRAY_HTTP_PORT: u16 = 25346;

// Reuse the desktop parser/config builder. A real `vless/mod.rs` is required:
// rustc and rustfmt resolve `#[path]` differently for inline modules.
#[path = "../../../../src/vless/link.rs"]
pub mod link;
