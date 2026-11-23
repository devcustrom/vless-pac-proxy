//! Защита от SSRF: запрещённые диапазоны адресов и проверка при подключении
//! (аналог checkedLookup из legacy/vless-pool.js).
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::{bail, Context, Result};

pub fn is_forbidden_ipv4(ip: Ipv4Addr) -> bool {
    let n = u32::from(ip);
    let in_range = |base: [u8; 4], mask: [u8; 4]| -> bool {
        let b = u32::from(Ipv4Addr::from(base));
        let m = u32::from(Ipv4Addr::from(mask));
        (n & m) == (b & m)
    };
    in_range([0, 0, 0, 0], [255, 0, 0, 0])          // 0/8
        || in_range([10, 0, 0, 0], [255, 0, 0, 0])   // 10/8
        || in_range([100, 64, 0, 0], [255, 192, 0, 0]) // CGNAT 100.64/10
        || in_range([127, 0, 0, 0], [255, 0, 0, 0])  // loopback
        || in_range([169, 254, 0, 0], [255, 255, 0, 0]) // link-local
        || in_range([172, 16, 0, 0], [255, 240, 0, 0]) // 172.16/12
        || in_range([192, 0, 0, 0], [255, 255, 255, 0]) // IETF
        || in_range([192, 168, 0, 0], [255, 255, 0, 0]) // private
        || in_range([198, 18, 0, 0], [255, 254, 0, 0]) // benchmarking
        || in_range([224, 0, 0, 0], [240, 0, 0, 0])  // multicast
        || in_range([240, 0, 0, 0], [240, 0, 0, 0]) // reserved
}

pub fn is_forbidden_ipv6(ip: Ipv6Addr) -> bool {
    let seg = ip.segments();
    if ip.is_unspecified() || ip.is_loopback() {
        return true;
    }
    // v4-mapped: ::ffff:a.b.c.d
    if seg[0] == 0 && seg[1] == 0 && seg[2] == 0 && seg[3] == 0 && seg[4] == 0 && seg[5] == 0xffff {
        let v4 = Ipv4Addr::new(
            ((seg[6] >> 8) & 0xff) as u8,
            (seg[6] & 0xff) as u8,
            ((seg[7] >> 8) & 0xff) as u8,
            (seg[7] & 0xff) as u8,
        );
        return is_forbidden_ipv4(v4);
    }
    if seg[0] & 0xfe00 == 0xfc00 {
        return true; // ULA fc00::/7
    }
    if seg[0] & 0xffc0 == 0xfe80 {
        return true; // link-local fe80::/10
    }
    if seg[0] & 0xff00 == 0xff00 {
        return true; // multicast
    }
    if seg[0] == 0x2001 && seg[1] == 0x0db8 {
        return true; // documentation
    }
    false
}

pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_ipv4(v4),
        IpAddr::V6(v6) => is_forbidden_ipv6(v6),
    }
}

fn mapped_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let seg = ip.segments();
    if seg[0] == 0 && seg[1] == 0 && seg[2] == 0 && seg[3] == 0 && seg[4] == 0 && seg[5] == 0xffff {
        return Some(Ipv4Addr::new(
            ((seg[6] >> 8) & 0xff) as u8,
            (seg[6] & 0xff) as u8,
            ((seg[7] >> 8) & 0xff) as u8,
            (seg[7] & 0xff) as u8,
        ));
    }
    None
}

/// Частные сети, которые могут быть доступны через OpenVPN/LAN. Их можно
/// разрешать только для доменов, явно добавленных пользователем в direct.
pub fn is_local_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let n = u32::from(v4);
            let in_range = |base: [u8; 4], mask: [u8; 4]| {
                let b = u32::from(Ipv4Addr::from(base));
                let m = u32::from(Ipv4Addr::from(mask));
                (n & m) == (b & m)
            };
            in_range([10, 0, 0, 0], [255, 0, 0, 0])
                || in_range([100, 64, 0, 0], [255, 192, 0, 0])
                || in_range([172, 16, 0, 0], [255, 240, 0, 0])
                || in_range([192, 168, 0, 0], [255, 255, 0, 0])
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = mapped_v4(v6) {
                return is_local_private_ip(IpAddr::V4(v4));
            }
            v6.segments()[0] & 0xfe00 == 0xfc00 // ULA
        }
    }
}

/// Для direct-доменов разрешаем частные сети, но по-прежнему запрещаем
/// loopback, link-local, multicast и прочие служебные диапазоны.
pub fn is_forbidden_direct_ip(ip: IpAddr) -> bool {
    is_forbidden_ip(ip) && !is_local_private_ip(ip)
}

pub fn forbidden_hostname(host: &str) -> bool {
    let h = host.trim().to_lowercase();
    h.is_empty() || h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local")
}

/// Проверка хоста до запроса: имя/литерал и все резолвленные адреса.
pub async fn check_host(host: &str) -> Result<()> {
    if forbidden_hostname(host) {
        bail!("запрещённый хост: {host}");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_forbidden_ip(ip) {
            bail!("запрещённый адрес хоста: {host}");
        }
        return Ok(());
    }
    resolve_checked(host).await.map(|_| ())
}

/// Резолв с проверкой каждого адреса (аналог checkedLookup).
pub async fn resolve_checked(host: &str) -> Result<Vec<IpAddr>> {
    resolve_with(host, is_forbidden_ip).await
}

/// Резолв для явно разрешённых direct-доменов: частные сети OpenVPN/LAN
/// допустимы, loopback и link-local — нет.
pub async fn resolve_direct(host: &str) -> Result<Vec<IpAddr>> {
    resolve_with(host, is_forbidden_direct_ip).await
}

async fn resolve_with(host: &str, forbidden: fn(IpAddr) -> bool) -> Result<Vec<IpAddr>> {
    if forbidden_hostname(host) {
        bail!("запрещённый хост: {host}");
    }
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host, 0))
        .await
        .with_context(|| format!("не удалось разрешить {host}"))?
        .map(|sa| sa.ip())
        .collect();
    if addrs.is_empty() {
        bail!("хост не разрешается: {host}");
    }
    for ip in &addrs {
        if forbidden(*ip) {
            bail!("{host} указывает на запрещённый адрес ({ip})");
        }
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.5.5",
            "169.254.169.254",
            "100.64.0.1",
            "224.0.0.1",
            "0.0.0.0",
        ] {
            assert!(
                is_forbidden_ip(s.parse().unwrap()),
                "{s} должен быть запрещён"
            );
        }
        for s in ["1.1.1.1", "8.8.8.8", "5.129.236.81"] {
            assert!(
                !is_forbidden_ip(s.parse().unwrap()),
                "{s} должен быть разрешён"
            );
        }
    }

    #[test]
    fn direct_exceptions_allow_private_but_not_loopback() {
        for s in ["10.1.2.3", "192.168.100.155", "172.16.5.5", "100.64.0.1"] {
            let ip = s.parse().unwrap();
            assert!(is_forbidden_ip(ip), "{s} должен быть запрещён по умолчанию");
            assert!(is_local_private_ip(ip), "{s} должен быть частным");
            assert!(!is_forbidden_direct_ip(ip), "{s} разрешён для direct");
        }
        for s in ["127.0.0.1", "169.254.169.254", "0.0.0.0"] {
            let ip = s.parse().unwrap();
            assert!(
                is_forbidden_direct_ip(ip),
                "{s} должен быть запрещён и direct"
            );
        }
    }

    #[test]
    fn v6_ranges() {
        for s in [
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd42::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                is_forbidden_ip(s.parse().unwrap()),
                "{s} должен быть запрещён"
            );
        }
        assert!(!is_forbidden_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn hostnames() {
        assert!(forbidden_hostname("localhost"));
        assert!(forbidden_hostname("api.localhost"));
        assert!(forbidden_hostname("printer.local"));
        assert!(!forbidden_hostname("example.com"));
    }
}
