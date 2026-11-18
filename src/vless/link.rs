//! Парсер vless:// и ss:// ссылок и генерация конфига xray (паритет с
//! legacy vless-to-xray.js: Reality/TLS/ws/grpc/flow, SS SIP002 + legacy).
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use url::Url;

#[derive(Debug, Clone)]
pub struct ParsedLink {
    pub protocol: String, // "vless" | "ss"
    pub address: String,
    pub port: u16,
    pub name: String,
    pub network: String,
    pub security: String,
    // vless
    pub id: String,
    pub flow: Option<String>,
    // vmess
    pub alter_id: u32,
    pub vmess_security: String,
    pub sni: Option<String>,
    pub fingerprint: Option<String>,
    pub public_key: Option<String>,
    pub short_id: Option<String>,
    pub path: Option<String>,
    pub service_name: Option<String>,
    pub host_header: Option<String>,
    pub mode: Option<String>,
    pub extra: Option<String>,
    pub alpn: Option<String>,
    // shadowsocks
    pub method: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LinkInfo {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub protocol: String,
    pub network: String,
    pub security: String,
}

fn uuid_ok(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

fn percent_decode(input: &str) -> String {
    percent_encoding::percent_decode_str(input)
        .decode_utf8_lossy()
        .to_string()
}

fn query_param(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.clone())
}

pub fn parse_vless(raw: &str) -> Result<ParsedLink> {
    let u = Url::parse(raw.trim()).context("некорректная vless-ссылка")?;
    if u.scheme() != "vless" {
        bail!("ссылка должна начинаться с vless://");
    }
    let id = u.username().to_string();
    if !uuid_ok(&id) {
        bail!("uuid в ссылке выглядит некорректно");
    }
    let address = u
        .host_str()
        .context("нет хоста в vless-ссылке")?
        .to_string();
    let port = u.port().context("нет порта в vless-ссылке")?;
    let name = u
        .fragment()
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| address.clone());
    let pairs: Vec<(String, String)> = u.query_pairs().into_owned().collect();

    let lower = |key: &str, def: Option<&str>| -> Option<String> {
        query_param(&pairs, key)
            .map(|v| v.to_lowercase())
            .filter(|v| !v.is_empty())
            .or_else(|| def.map(|d| d.to_string()))
    };

    let network = lower("type", Some("tcp")).unwrap_or_else(|| "tcp".into());
    let security = lower("security", Some("none")).unwrap_or_else(|| "none".into());
    Ok(ParsedLink {
        protocol: "vless".into(),
        address,
        port,
        name,
        network,
        security,
        id,
        flow: query_param(&pairs, "flow").filter(|v| !v.is_empty()),
        alter_id: 0,
        vmess_security: String::new(),
        sni: query_param(&pairs, "sni").or_else(|| query_param(&pairs, "host")),
        fingerprint: lower("fp", None),
        public_key: query_param(&pairs, "pbk"),
        short_id: query_param(&pairs, "sid"),
        path: query_param(&pairs, "path"),
        service_name: query_param(&pairs, "serviceName"),
        host_header: query_param(&pairs, "host"),
        mode: query_param(&pairs, "mode").filter(|v| !v.is_empty()),
        extra: query_param(&pairs, "extra").filter(|v| !v.is_empty()),
        alpn: query_param(&pairs, "alpn").filter(|v| !v.is_empty()),
        method: String::new(),
        password: String::new(),
    })
}

pub fn parse_ss(raw: &str) -> Result<ParsedLink> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("ss://")
        .or_else(|| trimmed.strip_prefix("SS://"))
        .context("ссылка должна начинаться с ss://")?;
    let (body, name_raw) = match rest.split_once('#') {
        Some((b, n)) => (b, Some(percent_decode(n))),
        None => (rest, None),
    };
    let body = body.split('?').next().unwrap_or(body); // plugin и пр. игнорируем

    let (method, password, address, port) =
        if let Some((userinfo, hostport)) = body.rsplit_once('@') {
            // SIP002: ss://base64(method:password)@host:port или
            // plaintext method:password (может быть percent-encoded).
            let decoded = if userinfo.contains(':') {
                percent_decode(userinfo)
            } else {
                decode_b64(userinfo).unwrap_or_else(|_| percent_decode(userinfo))
            };
            let (m, p) = decoded
                .split_once(':')
                .context("не удалось разобрать ss userinfo")?;
            let (host, port) = split_host_port(hostport)?;
            (m.to_string(), p.to_string(), host, port)
        } else {
            // legacy: ss://base64(method:password@host:port)
            let decoded = decode_b64(body).context("не удалось разобрать legacy ss-ссылку")?;
            let (m, rest2) = decoded
                .split_once(':')
                .context("не удалось разобрать legacy ss-ссылку")?;
            let (p, hostport) = rest2
                .rsplit_once('@')
                .context("не удалось разобрать legacy ss-ссылку")?;
            let (host, port) = split_host_port(hostport)?;
            (m.to_string(), p.to_string(), host, port)
        };

    Ok(ParsedLink {
        protocol: "ss".into(),
        address: address.clone(),
        port,
        name: name_raw.unwrap_or(address),
        network: "tcp".into(),
        security: "none".into(),
        id: String::new(),
        flow: None,
        alter_id: 0,
        vmess_security: String::new(),
        sni: None,
        fingerprint: None,
        public_key: None,
        short_id: None,
        path: None,
        service_name: None,
        host_header: None,
        mode: None,
        extra: None,
        alpn: None,
        method: method.to_lowercase(),
        password,
    })
}

/// vmess://base64(json) — формат v2rayN.
pub fn parse_vmess(raw: &str) -> Result<ParsedLink> {
    let trimmed = raw.trim();
    let payload = trimmed
        .strip_prefix("vmess://")
        .or_else(|| trimmed.strip_prefix("VMESS://"))
        .context("ссылка должна начинаться с vmess://")?;
    // Некоторые клиенты (Hiddify/v2rayN) добавляют query после base64:
    // vmess://<base64>?fp=edge
    let payload = payload.split('#').next().unwrap_or(payload);
    let (b64, query) = payload.split_once('?').unwrap_or((payload, ""));
    let query_pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let text = decode_b64(b64).context("не удалось разобрать vmess base64")?;
    let value: Value = serde_json::from_str(&text).context("vmess: некорректный JSON")?;
    let address = value
        .get("add")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .context("vmess: нет адреса")?
        .to_string();
    let port = value
        .get("port")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })
        .context("vmess: нет порта")?;
    if !(1..=65535).contains(&port) {
        bail!("vmess: порт должен быть 1..65535");
    }
    let port = port as u16;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !uuid_ok(&id) {
        bail!("vmess: uuid выглядит некорректно");
    }
    let alter_id = value
        .get("aid")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        })
        .unwrap_or(0) as u32;
    let network = value
        .get("net")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp")
        .to_lowercase();
    let network = if network == "raw" {
        "tcp".to_string()
    } else {
        network
    };
    let tls = value
        .get("tls")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    let security = if tls == "tls" { "tls" } else { "none" };
    let host = value
        .get("host")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let path = value
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let sni = query_param(&query_pairs, "sni")
        .filter(|v| !v.is_empty())
        .or_else(|| {
            value
                .get("sni")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        })
        .or_else(|| host.clone());
    let name = value
        .get("ps")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or(&address)
        .to_string();
    Ok(ParsedLink {
        protocol: "vmess".into(),
        address,
        port,
        name,
        network,
        security: security.to_string(),
        id,
        flow: None,
        alter_id,
        vmess_security: value
            .get("scy")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string(),
        sni,
        fingerprint: query_param(&query_pairs, "fp")
            .filter(|v| !v.is_empty())
            .or_else(|| {
                value
                    .get("fp")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            }),
        public_key: None,
        short_id: None,
        path,
        service_name: None,
        host_header: host,
        mode: None,
        extra: None,
        alpn: query_param(&query_pairs, "alpn")
            .filter(|v| !v.is_empty())
            .or_else(|| {
                value
                    .get("alpn")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            }),
        method: String::new(),
        password: String::new(),
    })
}

/// trojan://password@host:port?security=tls&type=ws&path=...&host=...
pub fn parse_trojan(raw: &str) -> Result<ParsedLink> {
    let u = Url::parse(raw.trim()).context("некорректная trojan-ссылка")?;
    if u.scheme() != "trojan" {
        bail!("ссылка должна начинаться с trojan://");
    }
    let password = percent_decode(u.username());
    if password.is_empty() {
        bail!("trojan: нет пароля");
    }
    let address = u.host_str().context("trojan: нет хоста")?.to_string();
    let port = u.port().context("trojan: нет порта")?;
    let name = u
        .fragment()
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| address.clone());
    let pairs: Vec<(String, String)> = u.query_pairs().into_owned().collect();
    let lower = |key: &str| -> Option<String> {
        query_param(&pairs, key)
            .map(|v| v.to_lowercase())
            .filter(|v| !v.is_empty())
    };
    let network = lower("type").unwrap_or_else(|| "tcp".into());
    let security = lower("security").unwrap_or_else(|| "tls".into());
    let host = query_param(&pairs, "host").filter(|v| !v.is_empty());
    let sni = query_param(&pairs, "sni")
        .filter(|v| !v.is_empty())
        .or_else(|| host.clone());
    Ok(ParsedLink {
        protocol: "trojan".into(),
        address,
        port,
        name,
        network,
        security,
        id: String::new(),
        flow: None,
        alter_id: 0,
        vmess_security: String::new(),
        sni,
        fingerprint: lower("fp"),
        public_key: None,
        short_id: None,
        path: query_param(&pairs, "path").filter(|v| !v.is_empty()),
        service_name: query_param(&pairs, "serviceName").filter(|v| !v.is_empty()),
        host_header: host,
        mode: lower("mode"),
        extra: query_param(&pairs, "extra").filter(|v| !v.is_empty()),
        alpn: lower("alpn"),
        method: String::new(),
        password,
    })
}

fn split_host_port(hostport: &str) -> Result<(String, u16)> {
    let (host, port) = hostport.rsplit_once(':').context("нет порта в ss-ссылке")?;
    let host = host.trim_matches(|c| c == '[' || c == ']').to_string();
    let port: u16 = port.parse().context("некорректный порт в ss-ссылке")?;
    Ok((host, port))
}

fn decode_b64(s: &str) -> Result<String> {
    use base64::Engine;
    let normalized = s.trim().replace('-', "+").replace('_', "/");
    let normalized = normalized.trim_end_matches('=');
    let padded = match normalized.len() % 4 {
        2 => format!("{normalized}=="),
        3 => format!("{normalized}="),
        _ => normalized.to_string(),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(padded.as_bytes())
        .context("base64 decode")?;
    String::from_utf8(bytes).context("base64 не UTF-8")
}

pub fn parse_any(raw: &str) -> Result<ParsedLink> {
    let s = raw.trim();
    if s.to_ascii_lowercase().starts_with("vless://") {
        parse_vless(s)
    } else if s.to_ascii_lowercase().starts_with("ss://") {
        parse_ss(s)
    } else if s.to_ascii_lowercase().starts_with("vmess://") {
        parse_vmess(s)
    } else if s.to_ascii_lowercase().starts_with("trojan://") {
        parse_trojan(s)
    } else {
        bail!("поддерживаются vless://, vmess://, trojan:// и ss:// ссылки");
    }
}

pub fn link_info(p: &ParsedLink) -> LinkInfo {
    LinkInfo {
        name: p.name.clone(),
        address: p.address.clone(),
        port: p.port,
        protocol: p.protocol.clone(),
        network: p.network.clone(),
        security: p.security.clone(),
    }
}

fn stream_settings(p: &ParsedLink) -> Value {
    let mut stream = json!({ "network": p.network, "security": p.security });
    if p.security == "reality" {
        stream["realitySettings"] = json!({
            "serverName": p.sni.clone().unwrap_or_default(),
            "fingerprint": p.fingerprint.clone().unwrap_or_else(|| "chrome".into()),
            "publicKey": p.public_key.clone().unwrap_or_default(),
            "shortId": p.short_id.clone().unwrap_or_default(),
            "spiderX": "/"
        });
    } else if p.security == "tls" {
        let mut tls = json!({
            "serverName": p.sni.clone().unwrap_or_default(),
            "fingerprint": p.fingerprint.clone().unwrap_or_else(|| "chrome".into()),
            "allowInsecure": false
        });
        if let Some(alpn) = &p.alpn {
            let values: Vec<&str> = alpn
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect();
            if !values.is_empty() {
                tls["alpn"] = json!(values);
            }
        }
        stream["tlsSettings"] = tls;
    }
    if p.network == "ws" {
        let mut ws = json!({ "path": p.path.clone().unwrap_or_else(|| "/".into()) });
        if let Some(host) = &p.host_header {
            ws["headers"] = json!({ "Host": host });
        }
        stream["wsSettings"] = ws;
    } else if p.network == "grpc" {
        stream["grpcSettings"] =
            json!({ "serviceName": p.service_name.clone().unwrap_or_default() });
    } else if p.network == "xhttp" {
        let mut xhttp = json!({
            "path": p.path.clone().unwrap_or_else(|| "/".into()),
            "host": p.host_header.clone().unwrap_or_default()
        });
        if let Some(mode) = &p.mode {
            xhttp["mode"] = json!(mode);
        }
        if let Some(extra) = &p.extra {
            xhttp["extra"] = serde_json::from_str::<Value>(extra).unwrap_or_else(|_| json!(extra));
        }
        stream["xhttpSettings"] = xhttp;
    } else if p.network == "httpupgrade" {
        let mut upgrade = json!({ "path": p.path.clone().unwrap_or_else(|| "/".into()) });
        if let Some(host) = &p.host_header {
            upgrade["host"] = json!(host);
        }
        stream["httpupgradeSettings"] = upgrade;
    }
    stream
}

/// Outbound для xray: vless, vmess, trojan или shadowsocks.
pub fn build_outbound(p: &ParsedLink, tag: &str) -> Value {
    let stream = stream_settings(p);
    match p.protocol.as_str() {
        "vless" => {
            let mut user = json!({ "id": p.id, "encryption": "none", "level": 0 });
            if p.security == "reality" {
                if let Some(flow) = &p.flow {
                    user["flow"] = json!(flow);
                }
            }
            json!({
                "tag": tag,
                "protocol": "vless",
                "settings": { "vnext": [{ "address": p.address, "port": p.port, "users": [user] }] },
                "streamSettings": stream,
                "mux": { "enabled": false, "concurrency": -1 }
            })
        }
        "vmess" => {
            let security = if p.vmess_security.is_empty() {
                "auto".to_string()
            } else {
                p.vmess_security.clone()
            };
            json!({
                "tag": tag,
                "protocol": "vmess",
                "settings": { "vnext": [{
                    "address": p.address,
                    "port": p.port,
                    "users": [{
                        "id": p.id,
                        "alterId": p.alter_id,
                        "security": security
                    }]
                }]},
                "streamSettings": stream,
                "mux": { "enabled": false, "concurrency": -1 }
            })
        }
        "trojan" => json!({
            "tag": tag,
            "protocol": "trojan",
            "settings": { "servers": [{
                "address": p.address,
                "port": p.port,
                "password": p.password
            }]},
            "streamSettings": stream,
            "mux": { "enabled": false, "concurrency": -1 }
        }),
        _ => json!({
            "tag": tag,
            "protocol": "shadowsocks",
            "settings": { "servers": [{
                "address": p.address,
                "port": p.port,
                "method": p.method,
                "password": p.password
            }]},
            "mux": { "enabled": false, "concurrency": -1 }
        }),
    }
}

/// Полный конфиг xray: HTTP-инбаунд 25346 + один outbound.
pub fn generate_config(raw: &str) -> Result<(Value, ParsedLink)> {
    let parsed = parse_any(raw)?;
    let outbound = build_outbound(
        &parsed,
        match parsed.protocol.as_str() {
            "vless" => "vless-out",
            "vmess" => "vmess-out",
            "trojan" => "trojan-out",
            _ => "ss-out",
        },
    );
    let config = json!({
        "log": { "loglevel": "warning" },
        "inbounds": [{
            "listen": "127.0.0.1",
            "port": crate::vless::XRAY_HTTP_PORT,
            "protocol": "http",
            "settings": { "allowTransparent": false }
        }],
        "outbounds": [outbound]
    });
    Ok((config, parsed))
}

/// Записывает xray-config.json (с .bak), возвращает краткую информацию.
pub fn save_link(dir: &Path, raw: &str) -> Result<LinkInfo> {
    let (config, parsed) = generate_config(raw)?;
    let target = crate::paths::in_dir(dir, "xray-config.json")?;
    if target.exists() {
        let _ = std::fs::copy(&target, dir.join("xray-config.json.bak"));
    }
    let tmp = dir.join("xray-config.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&config)?)?;
    std::fs::rename(&tmp, &target)?;
    Ok(link_info(&parsed))
}

pub fn has_config(dir: &Path) -> bool {
    dir.join("xray-config.json").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALITY: &str = "vless://9d8c3007-dfcd-4d34-a729-ae793f826581@189.74.115.23:443?encryption=none&flow=xtls-rprx-vision&fp=firefox&pbk=abcDEF123&security=reality&sid=6ba85179&sni=www.microsoft.com&type=tcp#Reality";

    #[test]
    fn parses_reality_with_flow() {
        let p = parse_vless(REALITY).unwrap();
        assert_eq!(p.address, "189.74.115.23");
        assert_eq!(p.port, 443);
        assert_eq!(p.network, "tcp");
        assert_eq!(p.security, "reality");
        assert_eq!(p.flow.as_deref(), Some("xtls-rprx-vision"));
        assert_eq!(p.short_id.as_deref(), Some("6ba85179"));
        let out = build_outbound(&p, "out0");
        assert_eq!(
            out["settings"]["vnext"][0]["users"][0]["flow"],
            "xtls-rprx-vision"
        );
        assert_eq!(
            out["streamSettings"]["realitySettings"]["publicKey"],
            "abcDEF123"
        );
    }

    #[test]
    fn parses_ws_tls() {
        let p = parse_vless("vless://a1b2c3d4-1111-2222-3333-444455556666@cdn.example.com:2053?security=tls&type=ws&path=%2Fws&host=front.example.com#WS").unwrap();
        assert_eq!(p.network, "ws");
        assert_eq!(p.path.as_deref(), Some("/ws"));
        assert_eq!(p.host_header.as_deref(), Some("front.example.com"));
        let out = build_outbound(&p, "out0");
        assert_eq!(out["streamSettings"]["wsSettings"]["path"], "/ws");
        assert_eq!(
            out["streamSettings"]["tlsSettings"]["serverName"],
            "front.example.com"
        );
    }

    #[test]
    fn parses_ss_sip002() {
        // a: b = base64("aes-256-gcm:pass")
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode("aes-256-gcm:pass")
        };
        let link = format!("ss://{b64}@1.2.3.4:8388#SS");
        let p = parse_ss(&link).unwrap();
        assert_eq!(p.method, "aes-256-gcm");
        assert_eq!(p.password, "pass");
        assert_eq!(p.address, "1.2.3.4");
        assert_eq!(p.port, 8388);
    }

    #[test]
    fn parses_ss_legacy() {
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .encode("chacha20-ietf-poly1305:secret@5.6.7.8:8389")
        };
        let p = parse_ss(&format!("ss://{b64}")).unwrap();
        assert_eq!(p.method, "chacha20-ietf-poly1305");
        assert_eq!(p.address, "5.6.7.8");
        assert_eq!(p.port, 8389);
    }

    #[test]
    fn parses_vmess() {
        let payload = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(
                r#"{"v":"2","ps":"VM","add":"1.2.3.4","port":"443","id":"a1b2c3d4-1111-2222-3333-444455556666","aid":"0","net":"ws","path":"/ws","host":"front.example.com","tls":"tls","scy":"auto"}"#,
            )
        };
        let p = parse_any(&format!("vmess://{payload}")).unwrap();
        assert_eq!(p.protocol, "vmess");
        assert_eq!(p.address, "1.2.3.4");
        assert_eq!(p.port, 443);
        assert_eq!(p.network, "ws");
        assert_eq!(p.security, "tls");
        assert_eq!(p.host_header.as_deref(), Some("front.example.com"));
        let out = build_outbound(&p, "out0");
        assert_eq!(out["protocol"], "vmess");
        assert_eq!(out["settings"]["vnext"][0]["users"][0]["id"], p.id);
    }

    #[test]
    fn parses_vmess_with_query_fp() {
        let payload = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(
                r#"{"v":"2","ps":"VM","add":"1.2.3.4","port":"443","id":"a1b2c3d4-1111-2222-3333-444455556666","aid":"0","net":"ws","path":"/ws","host":"front.example.com","tls":"tls","scy":"auto"}"#,
            )
        };
        let p = parse_vmess(&format!("vmess://{payload}?fp=edge")).unwrap();
        assert_eq!(p.network, "ws");
        assert_eq!(p.fingerprint.as_deref(), Some("edge"));
        let out = build_outbound(&p, "out0");
        assert_eq!(out["streamSettings"]["tlsSettings"]["fingerprint"], "edge");
    }

    #[test]
    fn parses_vmess_raw_as_tcp() {
        let payload = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(
                r#"{"v":"2","add":"1.2.3.4","port":443,"id":"a1b2c3d4-1111-2222-3333-444455556666","aid":0,"net":"raw","tls":"none"}"#,
            )
        };
        let p = parse_vmess(&format!("vmess://{payload}")).unwrap();
        assert_eq!(p.network, "tcp");
    }

    #[test]
    fn rejects_vmess_port_overflow() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"1.2.3.4","port":70000,"id":"a1b2c3d4-1111-2222-3333-444455556666","aid":0,"net":"tcp","tls":"none"}"#,
        );
        assert!(parse_vmess(&format!("vmess://{payload}")).is_err());
        let payload = base64::engine::general_purpose::STANDARD.encode(
            r#"{"v":"2","add":"1.2.3.4","port":0,"id":"a1b2c3d4-1111-2222-3333-444455556666","aid":0,"net":"tcp","tls":"none"}"#,
        );
        assert!(parse_vmess(&format!("vmess://{payload}")).is_err());
    }

    #[test]
    fn parses_ss_plain_userinfo() {
        let p =
            parse_ss("ss://2022-blake3-aes-256-gcm:pass%2Fword%3D@example.com:8388#SS").unwrap();
        assert_eq!(p.method, "2022-blake3-aes-256-gcm");
        assert_eq!(p.password, "pass/word=");
        assert_eq!(p.address, "example.com");
        assert_eq!(p.port, 8388);
    }

    #[test]
    fn parses_trojan() {
        let p = parse_any(
            "trojan://secret@example.com:443?security=tls&type=ws&path=%2Fws&host=front.example.com#T",
        )
        .unwrap();
        assert_eq!(p.protocol, "trojan");
        assert_eq!(p.password, "secret");
        assert_eq!(p.network, "ws");
        assert_eq!(p.security, "tls");
        assert_eq!(p.path.as_deref(), Some("/ws"));
        let out = build_outbound(&p, "out0");
        assert_eq!(out["protocol"], "trojan");
        assert_eq!(out["settings"]["servers"][0]["password"], "secret");
    }

    #[test]
    fn parses_xhttp() {
        let p = parse_any(
            "vless://a1b2c3d4-1111-2222-3333-444455556666@example.com:443?security=tls&type=xhttp&path=%2Fx&host=front.example.com&mode=auto#XHTTP",
        )
        .unwrap();
        assert_eq!(p.network, "xhttp");
        assert_eq!(p.path.as_deref(), Some("/x"));
        assert_eq!(p.mode.as_deref(), Some("auto"));
        let out = build_outbound(&p, "out0");
        assert_eq!(out["streamSettings"]["xhttpSettings"]["path"], "/x");
        assert_eq!(out["streamSettings"]["xhttpSettings"]["mode"], "auto");
    }

    #[test]
    fn rejects_bad_links() {
        assert!(parse_any("vmess://abc").is_err());
        assert!(parse_vless("vless://not-a-uuid@host:443").is_err());
    }
}
