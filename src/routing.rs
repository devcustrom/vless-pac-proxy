//! Маршрутизация по доменам (паритет с routeFor/availableChannel/activeChannel).
use std::collections::HashSet;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Wireguard,
    Vless,
}

pub struct Routing {
    pub proxied: HashSet<String>,
    pub direct_override: HashSet<String>,
    pub vless_only: HashSet<String>,
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
        let mut current = host.as_str();
        loop {
            if self.direct_override.contains(current) {
                return None;
            }
            if self.vless_only.contains(current) {
                return Some(self.available(Channel::Vless));
            }
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

    fn routing() -> Routing {
        let set =
            |items: &[&str]| -> HashSet<String> { items.iter().map(|s| s.to_string()).collect() };
        Routing {
            proxied: set(&["example.com", "claude.ai"]),
            direct_override: set(&["gosuslugi.ru"]),
            vless_only: set(&["vlessonly.test"]),
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
    }

    #[test]
    fn vless_only_and_fallbacks() {
        let mut r = routing();
        assert_eq!(r.route_for("vlessonly.test"), Some(Channel::Vless));
        // VLESS умер — VLESS-домены уходят на WireGuard
        r.vless_healthy = false;
        assert_eq!(r.route_for("vlessonly.test"), Some(Channel::Wireguard));
        // WireGuard умер, VLESS жив — обычные VPN-домены уходят на VLESS
        r.vless_healthy = true;
        r.wg_healthy = false;
        assert_eq!(r.route_for("example.com"), Some(Channel::Vless));
        // Оба мертвы — остаёмся на предпочтении (WireGuard)
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
