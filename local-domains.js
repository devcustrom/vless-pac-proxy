// Плагин локальных доменов: реверс-прокси по Host на локальные порты.
// Слушает только loopback; цели — фиксированный 127.0.0.1 + порт из конфига,
// поэтому наружу/в произвольные адреса не ходит. Поддерживает WebSocket-upgrade.
'use strict';
const http = require('http');
const net = require('net');

// Формат строки: "domain = 8080" или "domain 8080"
function parseLine(raw) {
  const line = String(raw || '').trim();
  if (!line || line.startsWith('#')) return null;
  const m = /^([^\s=]+)(?:\s*=\s*|\s+)(\d{1,5})$/.exec(line);
  if (!m) return null;
  const domain = m[1]
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/:\d+$/, '');
  const port = Number(m[2]);
  if (!domain || !(port >= 1 && port <= 65535)) return null;
  return { domain, port };
}

function parseConfig(text) {
  const out = [];
  const seen = new Set();
  for (const raw of String(text || '').split(/\r?\n/)) {
    const e = parseLine(raw);
    if (e && !seen.has(e.domain)) {
      seen.add(e.domain);
      out.push(e);
    }
  }
  return out;
}

function hostOnly(hostHeader) {
  return String(hostHeader || '').toLowerCase().replace(/:\d+$/, '');
}

function findTarget(domains, hostHeader) {
  const host = hostOnly(hostHeader);
  return domains.find((d) => d.domain === host) || null;
}

// ports: { port, getDomains, log }
function createLocalProxy({ port, getDomains, log }) {
  const server = http.createServer((req, res) => {
    const entry = findTarget(getDomains(), req.headers.host);
    if (!entry) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      const names = getDomains().map((d) => d.domain).join(', ') || '—';
      return res.end(`локальный домен не настроен: ${hostOnly(req.headers.host)}\nнастроены: ${names}`);
    }
    log(`local: ${hostOnly(req.headers.host)}${req.url} -> 127.0.0.1:${entry.port}`);
    const up = http.request(
      { host: '127.0.0.1', port: entry.port, method: req.method, path: req.url, headers: req.headers },
      (ures) => {
        res.writeHead(ures.statusCode || 502, ures.headers);
        ures.pipe(res);
      }
    );
    up.on('error', () => {
      try { res.writeHead(502); res.end('upstream error'); } catch { /* ignore */ }
    });
    req.pipe(up);
  });

  // WebSocket и прочие Upgrade — прозрачно, с сохранением исходных заголовков
  server.on('upgrade', (req, socket, head) => {
    const entry = findTarget(getDomains(), req.headers.host);
    if (!entry) {
      socket.destroy();
      return;
    }
    log(`local: upgrade ${hostOnly(req.headers.host)}${req.url} -> 127.0.0.1:${entry.port}`);
    const up = net.connect(entry.port, '127.0.0.1', () => {
      const lines = [String(req.method) + ' ' + String(req.url) + ' HTTP/1.1'];
      const raw = req.rawHeaders || [];
      for (let i = 0; i + 1 < raw.length; i += 2) {
        lines.push(raw[i] + ': ' + raw[i + 1]);
      }
      socket.write(lines.join('\r\n') + '\r\n\r\n');
      if (head && head.length) up.write(head);
      up.pipe(socket);
      socket.pipe(up);
    });
    up.on('error', () => socket.destroy());
    socket.on('error', () => up.destroy());
    up.on('close', () => socket.destroy());
    socket.on('close', () => up.destroy());
  });

  server.listen(port, '127.0.0.1');
  return server;
}

module.exports = { parseConfig, parseLine, createLocalProxy, hostOnly };
