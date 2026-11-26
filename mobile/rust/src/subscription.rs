//! Subscription text parsing for the mobile importer.

use crate::vless::link::{parse_any, ParsedLink};

/// Accepts plain text or a single base64-encoded subscription body.
pub fn parse_links(text: &str) -> Vec<String> {
    let mut body = text.to_string();
    if !body.contains("://") {
        use base64::Engine;
        let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes()) {
            if let Ok(decoded) = String::from_utf8(bytes) {
                body = decoded;
            }
        }
    }
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("vless://")
            || lower.starts_with("vmess://")
            || lower.starts_with("trojan://")
            || lower.starts_with("ss://")
        {
            out.push(line.to_string());
        }
    }
    out
}

/// Parses supported links and skips entries that are not valid for this app.
pub fn parse_profiles(text: &str) -> Vec<ParsedLink> {
    parse_links(text)
        .into_iter()
        .filter_map(|link| parse_any(&link).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_base64() {
        let plain = "# comment\nvless://9d8c3007-dfcd-4d34-a729-ae793f826581@1.2.3.4:443?security=tls#Node\n";
        assert_eq!(parse_links(plain).len(), 1);

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(plain.as_bytes());
        assert_eq!(parse_links(&encoded).len(), 1);
        assert_eq!(parse_profiles(&encoded).len(), 1);
    }
}
