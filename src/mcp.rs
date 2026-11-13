//! MCP-сервер (stdio JSON-RPC 2.0, без зависимостей): те же 5 инструментов, что
//! в legacy/mcp-server.js. Тонкий клиент к локальному API менеджера.
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

const API: &str = "127.0.0.1:8080";

fn http_call(method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
    let mut stream = TcpStream::connect(API).context("менеджер недоступен")?;
    let payload = body
        .map(|b| serde_json::to_string(b).unwrap_or_default())
        .unwrap_or_default();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {API}\r\nConnection: close\r\n");
    if !payload.is_empty() {
        req.push_str("Content-Type: application/json\r\nX-WPC: 1\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
    }
    req.push_str("\r\n");
    req.push_str(&payload);
    stream.write_all(req.as_bytes())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let (head, body) = resp
        .split_once("\r\n\r\n")
        .context("некорректный ответ менеджера")?;
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if status >= 400 {
        let msg = parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("ошибка API");
        anyhow::bail!("{msg}");
    }
    Ok(parsed)
}

fn tools() -> Value {
    json!([
        {
            "name": "local_domains_list",
            "description": "Список локальных доменов (domain -> port) и порты реверс-прокси (http/https).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "local_domains_add",
            "description": "Зарегистрировать локальный домен: http(s)://<domain> проксируется на 127.0.0.1:<port>.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "domain": { "type": "string" },
                    "port": { "type": "number" }
                },
                "required": ["domain", "port"]
            }
        },
        {
            "name": "local_domains_remove",
            "description": "Удалить локальный домен.",
            "inputSchema": {
                "type": "object",
                "properties": { "domain": { "type": "string" } },
                "required": ["domain"]
            }
        },
        {
            "name": "local_domains_probe",
            "description": "Проверить домен через реверс-прокси: статус и время ответа.",
            "inputSchema": {
                "type": "object",
                "properties": { "domain": { "type": "string" } },
                "required": ["domain"]
            }
        },
        {
            "name": "local_ports",
            "description": "Слушающие loopback-порты (найти порт dev-сервера).",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn call_tool(name: &str, args: &Value) -> Result<Value> {
    match name {
        "local_domains_list" => http_call("GET", "/api/local-domains", None),
        "local_domains_add" => {
            let domain = args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if domain.is_empty() {
                anyhow::bail!("нужен domain");
            }
            let port = args.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
            http_call(
                "POST",
                "/api/local-domains",
                Some(&json!({ "action": "add", "domain": domain, "port": port })),
            )
        }
        "local_domains_remove" => {
            let domain = args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if domain.is_empty() {
                anyhow::bail!("нужен domain");
            }
            http_call(
                "POST",
                "/api/local-domains",
                Some(&json!({ "action": "remove", "domain": domain })),
            )
        }
        "local_domains_probe" => {
            let domain = args
                .get("domain")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if domain.is_empty() {
                anyhow::bail!("нужен domain");
            }
            http_call(
                "POST",
                "/api/local-domains/probe",
                Some(&json!({ "domain": domain })),
            )
        }
        "local_ports" => http_call("GET", "/api/local-ports", None),
        other => anyhow::bail!("неизвестный инструмент: {other}"),
    }
}

fn respond(id: Option<&Value>, result: Option<Value>, error: Option<(i64, String)>) {
    let payload = match error {
        Some((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id.cloned().unwrap_or(Value::Null),
            "error": { "code": code, "message": message }
        }),
        None => json!({
            "jsonrpc": "2.0",
            "id": id.cloned().unwrap_or(Value::Null),
            "result": result.unwrap_or(Value::Null)
        }),
    };
    println!("{payload}");
    let _ = std::io::stdout().flush();
}

pub fn run_stdio(_dir: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = msg.get("id");
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        match method {
            "initialize" => respond(
                id,
                Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "wireproxy-local-domains", "version": env!("CARGO_PKG_VERSION") }
                })),
                None,
            ),
            "notifications/initialized" => {}
            "tools/list" => respond(id, Some(json!({ "tools": tools() })), None),
            "ping" => respond(id, Some(json!({})), None),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                match call_tool(name, &args) {
                    Ok(result) => respond(
                        id,
                        Some(json!({
                            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }]
                        })),
                        None,
                    ),
                    Err(e) => respond(
                        id,
                        Some(json!({
                            "content": [{ "type": "text", "text": format!("Ошибка: {e}") }],
                            "isError": true
                        })),
                        None,
                    ),
                }
            }
            _ => {
                if id.is_some() {
                    respond(
                        id,
                        None,
                        Some((-32601, format!("method not found: {method}"))),
                    );
                }
            }
        }
    }
    Ok(())
}
