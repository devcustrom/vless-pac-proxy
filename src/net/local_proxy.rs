//! Плагин локальных доменов: HTTP :80 + HTTPS :443 (переносится в M3).
use std::path::Path;

use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDomain {
    pub domain: String,
    pub port: u16,
}

/// Формат строки: "domain = port" или "domain port".
pub fn parse_line(raw: &str) -> Option<LocalDomain> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split(['=', ' ', '\t']).filter(|s| !s.is_empty());
    let domain = parts.next()?.to_lowercase();
    let port: u16 = parts.next()?.parse().ok()?;
    let domain = domain
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let domain = domain.split('/').next()?.split(':').next()?.to_string();
    if domain.is_empty() || port == 0 {
        return None;
    }
    Some(LocalDomain { domain, port })
}

pub fn parse_config(text: &str) -> Vec<LocalDomain> {
    let mut out: Vec<LocalDomain> = Vec::new();
    for line in text.lines() {
        if let Some(d) = parse_line(line) {
            if !out.iter().any(|x| x.domain == d.domain) {
                out.push(d);
            }
        }
    }
    out
}

pub async fn serve_http(_dir: &Path, _domains: Vec<LocalDomain>) -> Result<()> {
    anyhow::bail!("локальные домены ещё не перенесены (этап M3)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse_line("app.localhost = 3000"),
            Some(LocalDomain { domain: "app.localhost".into(), port: 3000 })
        );
        assert_eq!(
            parse_line("http://panel.localhost:8080/"),
            None
        );
        assert_eq!(
            parse_line("panel.localhost 8080"),
            Some(LocalDomain { domain: "panel.localhost".into(), port: 8080 })
        );
        assert_eq!(parse_line("# comment"), None);
    }

    #[test]
    fn dedup_and_order() {
        let cfg = "a.localhost = 1\n# x\nb.localhost = 2\na.localhost = 3\n";
        let d = parse_config(cfg);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].port, 1);
    }
}
