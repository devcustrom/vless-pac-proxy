//! Builds the Xray configuration used by the Android VPN service.
//!
//! The config is intentionally generated through `link.rs`/`wireguard.rs` so
//! desktop and mobile keep the same outbound semantics.

use serde::Serialize;
use serde_json::{json, Value};

use crate::vless::link::{build_outbound, ParsedLink};
use crate::wireguard::{build_outbound as build_wireguard_outbound, WireGuardConfig};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileConfigOptions {
    pub tun_mtu: u32,
    pub byedpi_port: Option<u16>,
    pub direct_domains: Vec<String>,
    pub dpi: bool,
}

impl Default for MobileConfigOptions {
    fn default() -> Self {
        Self {
            tun_mtu: 1500,
            byedpi_port: None,
            direct_domains: Vec::new(),
            dpi: true,
        }
    }
}

fn inbound(mtu: u32) -> Value {
    // Xray opens the TUN from the fd in `env.xray.tun.fd`; the name must stay
    // empty for Android because VpnService chooses it.
    json!({
        "port": 0,
        "protocol": "tun",
        "settings": { "name": "", "mtu": mtu },
        "sniffing": {
            "enabled": true,
            "destOverride": ["http", "tls", "quic"],
            "routeOnly": false
        }
    })
}

fn fragment_settings() -> Value {
    json!({
        "packets": "tlshello",
        "length": "100-200",
        "interval": "10-20"
    })
}

fn direct_outbound(dpi: bool) -> Value {
    let mut settings = json!({ "domainStrategy": "UseIP" });
    if dpi {
        settings["fragment"] = fragment_settings();
    }
    json!({
        "tag": "direct",
        "protocol": "freedom",
        "settings": settings
    })
}

fn byedpi_outbound(port: u16) -> Value {
    json!({
        "tag": "byedpi",
        "protocol": "socks",
        "settings": {
            "servers": [{ "address": "127.0.0.1", "port": port }]
        }
    })
}

fn direct_routing(options: &MobileConfigOptions) -> Value {
    let target = if options.dpi && options.byedpi_port.is_some() {
        "byedpi"
    } else {
        "direct"
    };
    let mut rules = vec![
        json!({
            "type": "field",
            "port": 53,
            "outboundTag": "dns-out"
        }),
        json!({
            "type": "field",
            "ip": [
                "10.0.0.0/8",
                "172.16.0.0/12",
                "192.168.0.0/16",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "::1/128",
                "fc00::/7",
                "fe80::/10"
            ],
            "outboundTag": "direct"
        }),
    ];
    if !options.direct_domains.is_empty() {
        rules.push(json!({
            "type": "field",
            "network": "tcp",
            "domain": options.direct_domains.iter().map(|domain| {
                format!("domain:{}", domain.trim_start_matches('.'))
            }).collect::<Vec<_>>(),
            "outboundTag": target
        }));
        rules.push(json!({
            "type": "field",
            "network": "udp",
            "domain": options.direct_domains.iter().map(|domain| format!("domain:{domain}")).collect::<Vec<_>>(),
            "outboundTag": "direct"
        }));
    }
    json!(rules)
}

fn base_config(proxy_outbound: Value, options: &MobileConfigOptions) -> Value {
    let mut outbounds = vec![
        proxy_outbound,
        direct_outbound(options.dpi),
        json!({ "tag": "dns-out", "protocol": "dns" }),
    ];
    if let Some(port) = options.byedpi_port.filter(|_| options.dpi) {
        outbounds.push(byedpi_outbound(port));
    }
    outbounds.push(json!({ "tag": "block", "protocol": "blackhole" }));

    json!({
        "log": { "loglevel": "warning" },
        "dns": {
            "servers": ["1.1.1.1", "8.8.8.8"],
            "queryStrategy": "UseIP"
        },
        "inbounds": [inbound(options.tun_mtu)],
        "outbounds": outbounds,
        "routing": {
            "domainStrategy": "IPIfNonMatch",
            "rules": direct_routing(options)
        }
    })
}

/// Xray config for a VLESS/VMess/Trojan/Shadowsocks profile.
pub fn build_vpn_config(profile: &ParsedLink, options: &MobileConfigOptions) -> Value {
    let tag = format!("{}-out", profile.protocol);
    base_config(build_outbound(profile, &tag), options)
}

/// Xray config for a WireGuard profile.
pub fn build_wireguard_vpn_config(
    profile: &WireGuardConfig,
    options: &MobileConfigOptions,
) -> Value {
    base_config(build_wireguard_outbound(profile, "wireguard-out"), options)
}

/// Serialized form ready for the Android Xray bridge.
pub fn to_json(config: &Value) -> String {
    serde_json::to_string(config).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vless::link::parse_any;

    const VLESS: &str = "vless://9d8c3007-dfcd-4d34-a729-ae793f826581@1.2.3.4:443?security=tls&type=ws&path=%2Fws&sni=example.com#Node";

    #[test]
    fn vless_config_has_tun_inbound_and_proxy() {
        let profile = parse_any(VLESS).unwrap();
        let config = build_vpn_config(
            &profile,
            &MobileConfigOptions {
                tun_mtu: 1500,
                byedpi_port: Some(31081),
                direct_domains: vec!["example.org".into()],
                dpi: true,
            },
        );
        assert_eq!(config["inbounds"][0]["protocol"], "tun");
        assert_eq!(config["inbounds"][0]["settings"]["mtu"], 1500);
        let outbounds = config["outbounds"].as_array().unwrap();
        assert!(outbounds.iter().any(|item| item["tag"] == "vless-out"));
        assert!(outbounds.iter().any(|item| item["tag"] == "dns-out"));
        assert!(outbounds.iter().any(|item| item["tag"] == "byedpi"));
        let rules = config["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|rule| rule["outboundTag"] == "dns-out"));
        assert!(rules.iter().any(|rule| rule["outboundTag"] == "byedpi"));
    }

    #[test]
    fn dpi_off_uses_direct() {
        let profile = parse_any(VLESS).unwrap();
        let config = build_vpn_config(
            &profile,
            &MobileConfigOptions {
                tun_mtu: 1500,
                byedpi_port: Some(31081),
                direct_domains: vec!["example.org".into()],
                dpi: false,
            },
        );
        let outbounds = config["outbounds"].as_array().unwrap();
        assert!(!outbounds.iter().any(|item| item["tag"] == "byedpi"));
        let rules = config["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().all(|rule| rule["outboundTag"] != "byedpi"));
    }

    #[test]
    fn wireguard_config_builds() {
        let raw = r#"
[Interface]
PrivateKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
Address = 10.0.0.2/32
[Peer]
PublicKey = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
Endpoint = vpn.example.com:51820
AllowedIPs = 0.0.0.0/0
"#;
        let wg = crate::wireguard::parse(raw).unwrap();
        let config = build_wireguard_vpn_config(&wg, &MobileConfigOptions::default());
        assert!(config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["tag"] == "wireguard-out"));
    }
}
