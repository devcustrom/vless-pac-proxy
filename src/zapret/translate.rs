//! Осторожный перевод типовых nfqws-стратегий (winws v1) в аргументы
//! zapret2/winws2. Неподдерживаемые профили пропускаются; если перевести
//! нечего — вызывающий код остаётся на старом движке.
use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct Translated {
    pub args: Vec<String>,
    pub converted_profiles: usize,
    pub skipped_profiles: usize,
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

fn split_profiles(args: &[String]) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new()];
    for arg in args {
        if arg == "--new" {
            out.push(Vec::new());
        } else {
            if let Some(last) = out.last_mut() {
                last.push(arg.clone());
            }
        }
    }
    out.retain(|profile| !profile.is_empty());
    out
}

fn tcp_ports(profile: &[String]) -> Vec<String> {
    let mut ports = Vec::new();
    for arg in profile {
        let Some(value) = arg.strip_prefix("--filter-tcp=") else {
            continue;
        };
        for part in value.split(',') {
            let part = part.trim();
            if !part.is_empty() && !ports.iter().any(|item| item == part) {
                ports.push(part.to_string());
            }
        }
    }
    ports
}

fn payload_for(ports: &[String]) -> Option<&'static str> {
    if ports.is_empty() {
        return None;
    }
    if ports.iter().all(|port| port == "80") {
        Some("http_req")
    } else if ports.iter().any(|port| port == "80") {
        // Смешанный профиль 80+443 требует двух разных payload — не рискуем.
        None
    } else {
        Some("tls_client_hello")
    }
}

fn lua_common(
    fooling: &[String],
    ttl: Option<&str>,
    autottl: Option<&str>,
    repeats: Option<&str>,
    ip_id_zero: bool,
) -> String {
    let mut out = String::new();
    for item in fooling {
        match item.as_str() {
            "md5sig" => out.push_str(":tcp_md5"),
            "ts" => out.push_str(":tcp_ts"),
            "badseq" => out.push_str(":tcp_seq=1000000"),
            "badsum" => out.push_str(":badsum"),
            _ => {}
        }
    }
    if let Some(value) = ttl {
        out.push_str(&format!(":ip_ttl={value}"));
    }
    if let Some(value) = autottl {
        out.push_str(&format!(":ip_autottl={value}"));
    }
    if let Some(value) = repeats {
        out.push_str(&format!(":repeats={value}"));
    }
    if ip_id_zero {
        out.push_str(":ip_id=zero");
    }
    out
}

fn blob_args(name: &str, value: &str, additions: &mut Vec<String>) -> Option<String> {
    let value = unquote(value);
    if value.is_empty() {
        return None;
    }
    if value.starts_with("0x") {
        return Some(value.to_string());
    }
    additions.push(format!("--blob={name}:@\"{value}\""));
    Some(name.to_string())
}

fn translate_tcp(profile: &[String]) -> Option<Vec<String>> {
    let ports = tcp_ports(profile);
    let payload = payload_for(&ports)?;
    let mut desync = Vec::new();
    let mut split_pos: Option<String> = None;
    let mut seqovl: Option<String> = None;
    let mut seqovl_pattern: Option<String> = None;
    let mut fake_tls: Option<String> = None;
    let mut fake_http: Option<String> = None;
    let mut faked_pattern: Option<String> = None;
    let mut fooling = Vec::new();
    let mut ttl = None;
    let mut autottl = None;
    let mut repeats = None;
    let mut ip_id_zero = false;
    let mut extras = Vec::new();
    let mut blobs = Vec::new();

    for arg in profile {
        if arg.starts_with("--filter-tcp=") {
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync=") {
            desync = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect();
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-split-pos=") {
            split_pos = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-split-seqovl=") {
            if !value.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            seqovl = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-split-seqovl-pattern=") {
            seqovl_pattern = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-fake-tls=") {
            if fake_tls.is_some() {
                return None;
            }
            fake_tls = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-fake-http=") {
            if fake_http.is_some() {
                return None;
            }
            fake_http = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-fakedsplit-pattern=") {
            faked_pattern = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-fooling=") {
            fooling = value
                .split(',')
                .map(str::trim)
                .map(ToString::to_string)
                .collect();
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-ttl=") {
            ttl = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-autottl=") {
            autottl = Some(value.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dpi-desync-repeats=") {
            repeats = Some(value.to_string());
            continue;
        }
        if arg == "--ip-id=zero" {
            ip_id_zero = true;
            continue;
        }
        if arg.starts_with("--hostlist=")
            || arg.starts_with("--hostlist-exclude=")
            || arg.starts_with("--hostlist-domains=")
            || arg.starts_with("--ipset=")
            || arg.starts_with("--ipset-exclude=")
        {
            extras.push(arg.clone());
            continue;
        }
        // Всё неизвестное (UDP/QUIC/any-protocol/cutoff/udplen и т.п.)
        // означает, что профиль не переводим.
        return None;
    }

    if desync.is_empty() {
        return None;
    }

    let mut out = vec![format!("--wf-tcp-out={}", ports.join(","))];
    out.push(format!("--payload={payload}"));
    out.extend(extras);

    let fake_value = if payload == "http_req" {
        fake_http.as_deref().or(fake_tls.as_deref())
    } else {
        fake_tls.as_deref().or(fake_http.as_deref())
    };
    let fake_blob = if let Some(value) = fake_value {
        blob_args("wpc_fake", value, &mut blobs).unwrap_or_else(|| {
            if payload == "http_req" {
                "fake_default_http".to_string()
            } else {
                "fake_default_tls".to_string()
            }
        })
    } else if payload == "http_req" {
        "fake_default_http".to_string()
    } else {
        "fake_default_tls".to_string()
    };
    let seq_blob = seqovl_pattern
        .as_deref()
        .and_then(|value| blob_args("wpc_seq", value, &mut blobs));

    out.extend(blobs);

    let common = lua_common(
        &fooling,
        ttl.as_deref(),
        autottl.as_deref(),
        repeats.as_deref(),
        ip_id_zero,
    );
    let pos = split_pos.unwrap_or_else(|| "midsld".to_string());
    let mut seq_suffix = String::new();
    if let Some(value) = seqovl {
        seq_suffix.push_str(&format!(":seqovl={value}"));
    } else if let Some(blob) = seq_blob {
        seq_suffix.push_str(&format!(":seqovl=#{blob}:seqovl_pattern={blob}"));
    }

    for token in desync {
        let lua = match token.as_str() {
            "fake" => format!("fake:blob={fake_blob}{common}"),
            "split" | "split2" => {
                format!("multisplit:pos={pos}{seq_suffix}{common}")
            }
            "disorder" | "disorder2" => {
                format!("multidisorder:pos={pos}{seq_suffix}{common}")
            }
            "multisplit" => format!("multisplit:pos={pos}{seq_suffix}{common}"),
            "multidisorder" => format!("multidisorder:pos={pos}{seq_suffix}{common}"),
            "fakedsplit" => {
                let pattern = faked_pattern
                    .as_deref()
                    .map(|value| format!(":pattern={value}"))
                    .unwrap_or_default();
                format!("fakedsplit:pos={pos}{pattern}{common}")
            }
            "fakeddisorder" => {
                let pattern = faked_pattern
                    .as_deref()
                    .map(|value| format!(":pattern={value}"))
                    .unwrap_or_default();
                format!("fakeddisorder:pos={pos}{pattern}{common}")
            }
            _ => return None,
        };
        out.push(format!("--lua-desync={lua}"));
    }
    Some(out)
}

/// Переводит стратегию целиком. UDP/QUIC- и L7-профили пропускаются.
pub fn translate_nfqws(args: &[String]) -> Result<Option<Translated>> {
    let profiles = split_profiles(args);
    let mut translated = Translated::default();
    for profile in profiles {
        if profile.iter().any(|arg| {
            arg.starts_with("--filter-udp=")
                || arg.starts_with("--filter-l7=")
                || arg.starts_with("--wf-udp")
        }) {
            translated.skipped_profiles += 1;
            continue;
        }
        if !profile.iter().any(|arg| arg.starts_with("--filter-tcp=")) {
            translated.skipped_profiles += 1;
            continue;
        }
        let Some(mut profile_args) = translate_tcp(&profile) else {
            translated.skipped_profiles += 1;
            continue;
        };
        if translated.converted_profiles > 0 {
            translated.args.push("--new".to_string());
        }
        translated.args.append(&mut profile_args);
        translated.converted_profiles += 1;
    }
    if translated.converted_profiles == 0 {
        return Ok(None);
    }
    translated.args.insert(0, "--wf-l3=ipv4".to_string());
    Ok(Some(translated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn translates_fake_and_split() {
        let input = args(&[
            "--filter-tcp=443",
            "--hostlist=C:/lists/general.txt",
            "--dpi-desync=fake,split2",
            "--dpi-desync-repeats=6",
            "--dpi-desync-fooling=md5sig",
            "--dpi-desync-split-pos=1",
            "--dpi-desync-fake-tls=C:/bin/tls.bin",
        ]);
        let out = translate_nfqws(&input).unwrap().unwrap();
        assert_eq!(out.converted_profiles, 1);
        assert_eq!(out.skipped_profiles, 0);
        let text = out.args.join(" ");
        assert!(text.contains("--wf-tcp-out=443"));
        assert!(text.contains("--payload=tls_client_hello"));
        assert!(text.contains("--blob=wpc_fake:@\"C:/bin/tls.bin\""));
        assert!(text.contains("--lua-desync=fake:blob=wpc_fake"));
        assert!(text.contains("--lua-desync=multisplit:pos=1"));
        assert!(text.contains(":tcp_md5"));
        assert!(text.contains(":repeats=6"));
        assert!(text.contains("--hostlist=C:/lists/general.txt"));
    }

    #[test]
    fn skips_udp_profiles() {
        let input = args(&[
            "--filter-udp=443",
            "--dpi-desync=fake",
            "--new",
            "--filter-tcp=443",
            "--dpi-desync=multisplit",
            "--dpi-desync-split-pos=midsld",
        ]);
        let out = translate_nfqws(&input).unwrap().unwrap();
        assert_eq!(out.converted_profiles, 1);
        assert_eq!(out.skipped_profiles, 1);
        assert!(out.args.join(" ").contains("--wf-tcp-out=443"));
    }

    #[test]
    fn translates_strategy_alt_profiles() {
        let input = args(&[
            "--filter-tcp=80",
            "--hostlist=C:/lists/general.txt",
            "--dpi-desync=fake,multisplit",
            "--dpi-desync-autottl=2",
            "--dpi-desync-fooling=md5sig",
            "--new",
            "--filter-tcp=2053,2083,2087,2096,8443",
            "--hostlist-domains=discord.media",
            "--dpi-desync=fake,fakedsplit",
            "--dpi-desync-repeats=6",
            "--dpi-desync-fooling=ts",
            "--dpi-desync-fakedsplit-pattern=0x00",
            "--dpi-desync-fake-tls=C:/bin/tls.bin",
            "--new",
            "--filter-udp=443",
            "--dpi-desync=fake",
            "--dpi-desync-repeats=6",
            "--dpi-desync-fake-quic=C:/bin/quic.bin",
            "--new",
            "--filter-tcp=443",
            "--hostlist=C:/lists/general.txt",
            "--dpi-desync=fake,fakedsplit",
            "--dpi-desync-repeats=6",
            "--dpi-desync-fooling=ts",
            "--dpi-desync-fakedsplit-pattern=0x00",
            "--dpi-desync-fake-tls=C:/bin/tls.bin",
        ]);
        let out = translate_nfqws(&input).unwrap().unwrap();
        assert_eq!(out.converted_profiles, 3);
        assert_eq!(out.skipped_profiles, 1);
        let text = out.args.join(" ");
        assert!(text.contains("--wf-tcp-out=80"));
        assert!(text.contains("--payload=http_req"));
        assert!(text.contains("--wf-tcp-out=2053,2083,2087,2096,8443"));
        assert!(text.contains("--lua-desync=fakedsplit:pos=midsld:pattern=0x00"));
        assert!(!text.contains("--filter-udp"));
    }

    #[test]
    fn rejects_unsupported_tcp_profile() {
        let input = args(&[
            "--filter-tcp=443",
            "--dpi-desync=fake",
            "--dpi-desync-any-protocol",
        ]);
        assert!(translate_nfqws(&input).unwrap().is_none());
    }
}
