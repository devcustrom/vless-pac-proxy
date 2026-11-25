//! Маршрутизация по доменам (паритет с routeFor/availableChannel/activeChannel).
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Wireguard,
    Vless,
}

/// Решение автопроверки для домена.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoChannel {
    Direct,
    Wireguard,
    Vless,
}

impl AutoChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Wireguard => "wireguard",
            Self::Vless => "vless",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "wireguard" => Some(Self::Wireguard),
            "vless" => Some(Self::Vless),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AutoChoice {
    pub channel: AutoChannel,
    pub via_zapret: bool,
    /// Более быстрый VPN-путь из последней проверки (для режима «всегда VPN»).
    pub vpn: Option<AutoChannel>,
}

pub struct Routing {
    pub proxied: HashSet<String>,
    pub direct_override: HashSet<String>,
    /// Домены, которые всегда идут через VPN, даже если прямой ответ 200.
    pub vpn_only: HashSet<String>,
    /// Домены, для которых автопроверка уже выбрала маршрут.
    pub auto: HashMap<String, AutoChoice>,
    pub vpn_default: Channel,
    pub wg_healthy: bool,
    pub vless_healthy: bool,
}

fn normalize(host_raw: &str) -> Option<String> {
    let host = host_raw.trim().to_lowercase().replace(['[', ']'], "");
    if host.is_empty() || !host.contains('.') || host.parse::<IpAddr>().is_ok() {
        return None;
    }
    Some(host)
}

impl Routing {
    /// Возвращает выбранный канал для домена (None = напрямую).
    pub fn route_for(&self, host_raw: &str) -> Option<Channel> {
        let host = normalize(host_raw)?;
        // Исключения «Напрямую» действуют на все поддомены, даже если для
        // конкретного поддомена есть auto-решение.
        if self.is_direct_override(&host) {
            return None;
        }
        if let Some(route) = self.vpn_only_channel(&host) {
            return Some(route);
        }
        if let Some(choice) = self.auto_for(&host) {
            return match choice.channel {
                AutoChannel::Direct => None,
                AutoChannel::Wireguard => Some(self.available(Channel::Wireguard)),
                AutoChannel::Vless => Some(self.available(Channel::Vless)),
            };
        }
        let mut current = host.as_str();
        loop {
            if self.proxied.contains(current) {
                return Some(self.available(self.vpn_default));
            }
            match current.find('.') {
                Some(i) => current = &current[i + 1..],
                None => break,
            }
        }
        None
    }

    /// Наиболее специфичное auto-решение для домена.
    pub fn auto_for(&self, host_raw: &str) -> Option<AutoChoice> {
        let host = normalize(host_raw)?;
        let mut current = host.as_str();
        loop {
            if let Some(choice) = self.auto.get(current) {
                return Some(*choice);
            }
            match current.find('.') {
                Some(i) => current = &current[i + 1..],
                None => break,
            }
        }
        None
    }

    /// Канал для домена, закреплённого за VPN: более быстрый VPN из последней
    /// проверки, иначе основной канал.
    pub fn vpn_only_channel(&self, host_raw: &str) -> Option<Channel> {
        if !self.is_vpn_only(host_raw) {
            return None;
        }
        let pref = self
            .auto_for(host_raw)
            .and_then(|choice| choice.vpn)
            .unwrap_or(AutoChannel::Direct);
        let channel = match pref {
            AutoChannel::Wireguard => Channel::Wireguard,
            AutoChannel::Vless => Channel::Vless,
            AutoChannel::Direct => self.vpn_default,
        };
        Some(self.available(channel))
    }

    /// Домен закреплён за VPN в обход авто-выбора.
    pub fn is_vpn_only(&self, host_raw: &str) -> bool {
        let Some(host) = normalize(host_raw) else {
            return false;
        };
        let mut current = host.as_str();
        loop {
            if self.vpn_only.contains(current) {
                return true;
            }
            match current.find('.') {
                Some(i) => current = &current[i + 1..],
                None => break,
            }
        }
        false
    }

    /// Домен явно добавлен в исключения (напрямую / OpenVPN / LAN).
    pub fn is_direct_override(&self, host_raw: &str) -> bool {
        let Some(host) = normalize(host_raw) else {
            return false;
        };
        let mut current = host.as_str();
        loop {
            if self.direct_override.contains(current) {
                return true;
            }
            match current.find('.') {
                Some(i) => current = &current[i + 1..],
                None => break,
            }
        }
        false
    }

    /// Канал с учётом здоровья: мёртвый выбранный заменяется живым.
    pub fn available(&self, pref: Channel) -> Channel {
        match pref {
            Channel::Vless => {
                if self.vless_healthy || !self.wg_healthy {
                    Channel::Vless
                } else {
                    Channel::Wireguard
                }
            }
            Channel::Wireguard => {
                if self.wg_healthy || !self.vless_healthy {
                    Channel::Wireguard
                } else {
                    Channel::Vless
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn auto(items: &[(&str, AutoChannel)]) -> HashMap<String, AutoChoice> {
        items
            .iter()
            .map(|(domain, channel)| {
                (
                    domain.to_string(),
                    AutoChoice {
                        channel: *channel,
                        via_zapret: false,
                        vpn: None,
                    },
                )
            })
            .collect()
    }

    fn routing() -> Routing {
        Routing {
            proxied: set(&["example.com", "claude.ai", "chatgpt.com"]),
            direct_override: set(&["gosuslugi.ru"]),
            vpn_only: set(&["vpn-only.test"]),
            auto: auto(&[
                ("chatgpt.com", AutoChannel::Vless),
                ("direct-auto.test", AutoChannel::Direct),
            ]),
            vpn_default: Channel::Wireguard,
            wg_healthy: true,
            vless_healthy: true,
        }
    }

    #[test]
    fn suffix_matching() {
        let r = routing();
        assert_eq!(r.route_for("a.b.example.com"), Some(Channel::Wireguard));
        assert_eq!(r.route_for("claude.ai"), Some(Channel::Wireguard));
        assert_eq!(r.route_for("unrelated.org"), None);
    }

    #[test]
    fn direct_override_wins() {
        let mut r = routing();
        r.proxied.insert("gosuslugi.ru".into());
        assert_eq!(r.route_for("www.gosuslugi.ru"), None);
        assert!(r.is_direct_override("www.gosuslugi.ru"));
        assert!(!r.is_direct_override("example.com"));
    }

    #[test]
    fn vpn_only_beats_auto_and_direct_choice() {
        let mut r = routing();
        r.auto.insert(
            "vpn-only.test".into(),
            AutoChoice {
                channel: AutoChannel::Direct,
                via_zapret: false,
                vpn: Some(AutoChannel::Vless),
            },
        );
        assert_eq!(r.route_for("sub.vpn-only.test"), Some(Channel::Vless));
        r.vless_healthy = false;
        assert_eq!(r.route_for("vpn-only.test"), Some(Channel::Wireguard));
    }

    #[test]
    fn direct_override_beats_vpn_only() {
        let mut r = routing();
        r.vpn_only.insert("gosuslugi.ru".into());
        assert_eq!(r.route_for("www.gosuslugi.ru"), None);
    }

    #[test]
    fn auto_choice_wins_over_proxied() {
        let r = routing();
        assert_eq!(r.route_for("chatgpt.com"), Some(Channel::Vless));
        assert_eq!(r.route_for("ab.chatgpt.com"), Some(Channel::Vless));
        assert_eq!(r.route_for("direct-auto.test"), None);
    }

    #[test]
    fn direct_override_beats_auto_for_subdomain() {
        let mut r = routing();
        r.auto.insert(
            "sub.example.com".into(),
            AutoChoice {
                channel: AutoChannel::Vless,
                via_zapret: false,
                vpn: None,
            },
        );
        r.direct_override.insert("example.com".into());
        assert_eq!(r.route_for("sub.example.com"), None);
    }

    #[test]
    fn auto_health_fallbacks() {
        let mut r = routing();
        assert_eq!(r.route_for("chatgpt.com"), Some(Channel::Vless));
        // VLESS умер — auto-домен уходит на WireGuard.
        r.vless_healthy = false;
        assert_eq!(r.route_for("chatgpt.com"), Some(Channel::Wireguard));
        // Оба мертвы — остаётся выбранный канал.
        r.wg_healthy = false;
        assert_eq!(r.route_for("chatgpt.com"), Some(Channel::Vless));
    }

    #[test]
    fn default_health_fallback() {
        let mut r = routing();
        r.wg_healthy = false;
        assert_eq!(r.route_for("example.com"), Some(Channel::Vless));
        r.vless_healthy = false;
        assert_eq!(r.route_for("example.com"), Some(Channel::Wireguard));
    }

    #[test]
    fn ips_and_plain_hosts_are_direct() {
        let r = routing();
        assert_eq!(r.route_for("127.0.0.1"), None);
        assert_eq!(r.route_for("localhost"), None);
    }
}
