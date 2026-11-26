//! Parsing of WireGuard `.conf` files for the mobile app.
//!
//! The mobile app uses Xray's WireGuard outbound, so this module only turns a
//! standard client config into the subset needed by `build_wireguard_outbound`.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireGuardPeer {
    pub public_key: String,
    pub preshared_key: Option<String>,
    pub endpoint: String,
    pub allowed_ips: Vec<String>,
    pub keep_alive: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireGuardConfig {
    pub private_key: String,
    pub address: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: u32,
    pub peers: Vec<WireGuardPeer>,
}

fn split_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalize_address(value: &str) -> String {
    let value = value.trim();
    if value.contains('/') {
        return value.to_string();
    }
    if value.contains(':') {
        format!("{value}/128")
    } else {
        format!("{value}/32")
    }
}

fn valid_key(value: &str) -> bool {
    use base64::Engine;
    let trimmed = value.trim();
    if trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    let raw = trimmed.trim_end_matches('=');
    let decoded = if raw.contains('+') || raw.contains('/') {
        base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw)
    } else {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(raw)
    };
    decoded.map(|bytes| bytes.len() == 32).unwrap_or(false)
}

fn endpoint(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("WireGuard: пустой Endpoint");
    }
    if value.starts_with('[') {
        let close = value
            .find(']')
            .context("WireGuard: некорректный IPv6 Endpoint")?;
        let host = &value[1..close];
        let port = value[close + 1..]
            .strip_prefix(':')
            .context("WireGuard: в Endpoint нет порта")?;
        if host.is_empty() || port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
            bail!("WireGuard: некорректный Endpoint");
        }
        return Ok(value.to_string());
    }
    let (host, port) = value
        .rsplit_once(':')
        .context("WireGuard: в Endpoint нет порта")?;
    if host.is_empty() || port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
        bail!("WireGuard: некорректный Endpoint");
    }
    Ok(value.to_string())
}

/// Parses a standard `wg.conf`/WireGuard client configuration.
pub fn parse(raw: &str) -> Result<WireGuardConfig> {
    let mut section = "";
    let mut private_key = String::new();
    let mut address = Vec::new();
    let mut dns = Vec::new();
    let mut mtu = 1420u32;
    let mut peers = Vec::new();
    let mut peer: Option<WireGuardPeer> = None;

    for raw_line in raw.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(previous) = peer.take() {
                peers.push(previous);
            }
            section = line[1..line.len() - 1].trim();
            if section.eq_ignore_ascii_case("Peer") {
                peer = Some(WireGuardPeer {
                    public_key: String::new(),
                    preshared_key: None,
                    endpoint: String::new(),
                    allowed_ips: Vec::new(),
                    keep_alive: 0,
                });
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if section.eq_ignore_ascii_case("Interface") {
            match key.to_ascii_lowercase().as_str() {
                "privatekey" => private_key = value.to_string(),
                "address" => address.extend(
                    split_values(value)
                        .into_iter()
                        .map(|v| normalize_address(&v)),
                ),
                "dns" => dns.extend(split_values(value)),
                "mtu" => {
                    if let Ok(parsed) = value.parse() {
                        mtu = parsed;
                    }
                }
                _ => {}
            }
        } else if section.eq_ignore_ascii_case("Peer") {
            let Some(peer) = peer.as_mut() else { continue };
            match key.to_ascii_lowercase().as_str() {
                "publickey" => peer.public_key = value.to_string(),
                "presharedkey" => peer.preshared_key = Some(value.to_string()),
                "endpoint" => peer.endpoint = value.to_string(),
                "allowedips" => peer.allowed_ips.extend(split_values(value)),
                "persistentkeepalive" => {
                    peer.keep_alive = value.parse().unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    if let Some(previous) = peer.take() {
        peers.push(previous);
    }

    if private_key.is_empty() || !valid_key(&private_key) {
        bail!("WireGuard: отсутствует или повреждён PrivateKey");
    }
    if address.is_empty() {
        bail!("WireGuard: отсутствует Address");
    }
    if peers.is_empty() {
        bail!("WireGuard: нет секции Peer");
    }
    for peer in &mut peers {
        if peer.public_key.is_empty() || !valid_key(&peer.public_key) {
            bail!("WireGuard: отсутствует или повреждён PublicKey");
        }
        if peer
            .preshared_key
            .as_deref()
            .is_some_and(|key| !valid_key(key))
        {
            bail!("WireGuard: повреждён PresharedKey");
        }
        peer.endpoint = endpoint(&peer.endpoint)?;
        if peer.allowed_ips.is_empty() {
            peer.allowed_ips = vec!["0.0.0.0/0".to_string(), "::0/0".to_string()];
        }
    }

    Ok(WireGuardConfig {
        private_key,
        address,
        dns,
        mtu,
        peers,
    })
}

/// Builds the `settings` object for the Xray WireGuard outbound.
pub fn build_outbound(config: &WireGuardConfig, tag: &str) -> Value {
    let peers: Vec<Value> = config
        .peers
        .iter()
        .map(|peer| {
            let mut item = json!({
                "publicKey": peer.public_key,
                "endpoint": peer.endpoint,
                "allowedIPs": peer.allowed_ips,
            });
            if let Some(key) = &peer.preshared_key {
                item["preSharedKey"] = json!(key);
            }
            if peer.keep_alive > 0 {
                item["keepAlive"] = json!(peer.keep_alive);
            }
            item
        })
        .collect();

    let mut settings = json!({
        "secretKey": config.private_key,
        "address": config.address,
        "peers": peers,
        "mtu": config.mtu,
        "noKernelTun": true,
    });
    if !config.dns.is_empty() {
        settings["remoteDNS"] = json!(config.dns);
    }
    json!({
        "tag": tag,
        "protocol": "wireguard",
        "settings": settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[Interface]
PrivateKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
Address = 10.0.0.2/32, fd00::2/128
DNS = 1.1.1.1, 8.8.8.8
MTU = 1380

[Peer]
PublicKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
Endpoint = vpn.example.com:51820
AllowedIPs = 0.0.0.0/0, ::/0
PersistentKeepalive = 25
"#;

    #[test]
    fn parses_client_config() {
        let cfg = parse(CONFIG).unwrap();
        assert_eq!(cfg.mtu, 1380);
        assert_eq!(cfg.address.len(), 2);
        assert_eq!(cfg.peers.len(), 1);
        assert_eq!(cfg.peers[0].endpoint, "vpn.example.com:51820");
        assert_eq!(cfg.peers[0].keep_alive, 25);
        let out = build_outbound(&cfg, "wg-out");
        assert_eq!(out["protocol"], "wireguard");
        assert_eq!(out["settings"]["noKernelTun"], true);
    }

    #[test]
    fn rejects_bad_key() {
        let broken = CONFIG.replace(
            "PrivateKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            "PrivateKey = nope",
        );
        assert!(parse(&broken).is_err());
    }

    #[test]
    fn normalizes_addresses_and_ipv6_endpoint() {
        let config = CONFIG
            .replace("10.0.0.2/32, fd00::2/128", "10.0.0.2")
            .replace("vpn.example.com:51820", "[fd00::1]:51820");
        let cfg = parse(&config).unwrap();
        assert_eq!(cfg.address, ["10.0.0.2/32"]);
        assert_eq!(cfg.peers[0].endpoint, "[fd00::1]:51820");
    }
}
