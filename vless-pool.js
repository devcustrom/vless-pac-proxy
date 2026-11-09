// Пул конфигов из подписок: загрузка (с проверкой URL), парсинг vless:// и ss://,
// тестирование работоспособности через один тестовый процесс xray.
// Используется manager.js. Ограничение безопасности: качаем только http/https
// и никогда не ходим на localhost/приватные/зарезервированные адреса.
// Проверка каналов идёт встроенным HTTP-клиентом Node (через локальный порт
// тестового xray), без вызова внешних утилит.
'use strict';
const fs = require('fs');
const path = require('path');
const net = require('net');
const dns = require('dns');
const http = require('http');
const tls = require('tls');
const { spawn, execFile } = require('child_process');
const { generateLinkConfig } = require('./vless-to-xray');

const DIR = __dirname;
const IS_WIN = process.platform === 'win32';
const TEST_BASE_PORT = 26100; // диапазон локальных inbounds тестового xray
const MAX_TEST = 120;         // сколько конфигов проверять за один прогон
const PROBE_TIMEOUT_MS = 8000;

// Пути к исполняемому файлу и временным конфигам фиксированы и проверяются:
// только внутри каталога проекта (защита от выхода за границы).
function withinDir(p) {
  const root = path.resolve(DIR) + path.sep;
  return path.resolve(p).startsWith(root);
}
const XRAY_EXE_PATH = path.join(DIR, IS_WIN ? 'xray.exe' : 'xray');
const ONE_CFG_FILE = 'xray-pool-one.json';
const POOL_CFG_FILE = 'xray-pool-test.json';
const ONE_CFG_PATH = path.join(DIR, ONE_CFG_FILE);
const POOL_CFG_PATH = path.join(DIR, POOL_CFG_FILE);
if (!withinDir(XRAY_EXE_PATH) || !withinDir(ONE_CFG_PATH) || !withinDir(POOL_CFG_PATH)) {
  throw new Error('некорректные пути к компонентам xray');
}

// ---------------- безопасность URL ----------------
function ipv4ToInt(ip) {
  return ip.split('.').reduce((acc, x) => (acc << 8) + Number(x), 0) >>> 0;
}

function isForbiddenV4(ip) {
  const n = ipv4ToInt(ip);
  const inRange = (base, mask) => (n & mask) === (ipv4ToInt(base) & mask);
  return (
    inRange('0.0.0.0', 0xff000000) ||      // 0/8
    inRange('10.0.0.0', 0xff000000) ||     // 10/8
    inRange('100.64.0.0', 0xffc00000) ||   // CGNAT 100.64/10
    inRange('127.0.0.0', 0xff000000) ||    // loopback
    inRange('169.254.0.0', 0xffff0000) ||  // link-local
    inRange('172.16.0.0', 0xfff00000) ||   // 172.16/12
    inRange('192.0.0.0', 0xffffff00) ||    // IETF
    inRange('192.168.0.0', 0xffff0000) ||  // private
    inRange('198.18.0.0', 0xfffe0000) ||   // benchmarking
    inRange('224.0.0.0', 0xf0000000) ||    // multicast
    inRange('240.0.0.0', 0xf0000000)       // reserved
  );
}

function isForbiddenIp(ip) {
  const fam = net.isIP(ip);
  if (fam === 4) return isForbiddenV4(ip);
  if (fam === 6) {
    const v = ip.toLowerCase();
    if (v === '::' || v === '::1') return true;
    if (v.startsWith('::ffff:')) return isForbiddenV4(v.slice(7)); // v4-mapped
    if (/^(fc|fd)/.test(v)) return true;  // ULA fc00::/7
    if (/^fe[89ab]/.test(v)) return true; // link-local fe80::/10
    if (/^ff/.test(v)) return true;       // multicast
    if (v.startsWith('2001:db8')) return true; // documentation
    return false;
  }
  return true;
}

// Проверка URL перед серверным запросом: только http/https, хост резолвится
// и не является локальным/приватным/зарезервированным.
async function validateUrl(raw) {
  let u;
  try { u = new URL(raw); } catch { throw new Error('некорректный URL'); }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new Error('разрешены только http/https');
  const host = u.hostname.replace(/^\[|\]$/g, '');
  if (!host || host === 'localhost' || host.endsWith('.localhost') || host.endsWith('.local')) {
    throw new Error('запрещённый хост');
  }
  if (net.isIP(host)) {
    if (isForbiddenIp(host)) throw new Error('запрещённый адрес хоста');
    return u;
  }
  let addrs;
  try {
    addrs = await dns.promises.lookup(host, { all: true });
  } catch {
    throw new Error('не удалось разрешить хост');
  }
  if (!addrs.length) throw new Error('хост не разрешается');
  for (const a of addrs) {
    if (isForbiddenIp(a.address)) throw new Error('хост указывает на запрещённый адрес');
  }
  return u;
}

// Загрузка текста с ручной проверкой каждого редиректа
async function safeFetchText(raw, maxHops = 4) {
  let u = await validateUrl(raw);
  for (let hop = 0; hop <= maxHops; hop++) {
    const res = await fetch(u.href, {
      redirect: 'manual',
      signal: AbortSignal.timeout(30000),
      headers: { 'user-agent': 'wireproxy-control/1.0' },
    });
    if ([301, 302, 303, 307, 308].includes(res.status)) {
      const loc = res.headers.get('location');
      if (!loc) throw new Error('редирект без location');
      u = await validateUrl(new URL(loc, u).href);
      continue;
    }
    if (!res.ok) throw new Error('HTTP ' + res.status);
    const text = await res.text();
    if (text.length > 5 * 1024 * 1024) throw new Error('ответ слишком большой');
    return text;
  }
  throw new Error('слишком много редиректов');
}

// ---------------- парсинг подписки ----------------
function parseSubscriptionLinks(text) {
  let t = String(text || '');
  // стандартная подписка может быть целиком в base64
  if (!/:\/\//.test(t)) {
    try { t = Buffer.from(t.replace(/\s+/g, ''), 'base64').toString('utf8'); } catch { /* ignore */ }
  }
  const out = [];
  for (const line of t.split(/\r?\n/)) {
    const s = line.trim();
    if (!s || s.startsWith('#')) continue;
    if (/^(vless|ss):\/\//i.test(s)) out.push(s);
  }
  return out;
}

// ---------------- проверка каналов ----------------
function portUp(port) {
  return new Promise((resolve) => {
    const s = net.connect({ host: '127.0.0.1', port });
    s.setTimeout(400);
    s.on('connect', () => { s.destroy(); resolve(true); });
    s.on('error', () => resolve(false));
    s.on('timeout', () => { s.destroy(); resolve(false); });
  });
}

// Проверка канала через локальный порт xray: CONNECT + TLS + GET https://api.ipify.org
// (plain HTTP не годится: ipify отдаёт 520/редирект, и проба ложно падает).
function probeViaPort(port) {
  return new Promise((resolve) => {
    const started = Date.now();
    let done = false;
    const finish = (ok, ip) => {
      if (done) return;
      done = true;
      resolve(ok ? { ok: true, ms: Date.now() - started, ip } : { ok: false, ms: null });
    };
    const req = http.request({ host: '127.0.0.1', port, method: 'CONNECT', path: 'api.ipify.org:443' });
    req.setTimeout(PROBE_TIMEOUT_MS, () => { req.destroy(); finish(false); });
    req.on('error', () => finish(false));
    req.on('connect', (res, socket) => {
      if (res.statusCode !== 200) {
        socket.destroy();
        return finish(false);
      }
      const t = tls.connect({ socket, servername: 'api.ipify.org' }, () => {
        t.write('GET / HTTP/1.1\r\nHost: api.ipify.org\r\nUser-Agent: probe\r\nConnection: close\r\n\r\n');
      });
      let buf = '';
      t.setEncoding('utf8');
      t.on('data', (d) => { buf += d; if (buf.length > 8192) t.destroy(); });
      t.on('end', () => {
        const m = /\r\n\r\n([\s\S]*)$/.exec(buf);
        const body = m ? m[1].trim() : '';
        const ip = /^\d{1,3}(\.\d{1,3}){3}$/.test(body) ? body : null;
        finish(!!ip, ip);
      });
      t.setTimeout(PROBE_TIMEOUT_MS, () => { t.destroy(); finish(false); });
      t.on('error', () => finish(false));
    });
    req.end();
  });
}

function waitForPort(ports, deadlineMs) {
  const started = Date.now();
  return new Promise((resolve) => {
    const tick = async () => {
      if (Date.now() - started > deadlineMs) return resolve(false);
      const results = await Promise.all(ports.slice(0, 5).map(portUp));
      if (results.some(Boolean)) return resolve(true);
      setTimeout(tick, 150);
    };
    tick();
  });
}

// Проверка, что xray принимает конфиг одного конфига (xray -test).
// Один битый конфиг в общем тестовом файле обрушил бы весь прогон.
function validateOne(link) {
  let cfg;
  try {
    cfg = generateLinkConfig(link).config;
  } catch {
    return Promise.resolve(false);
  }
  cfg.log = { loglevel: 'none' };
  cfg.inbounds = [{ listen: '127.0.0.1', port: TEST_BASE_PORT, protocol: 'http' }];
  cfg.outbounds[0].tag = 'out0';
  try { fs.writeFileSync(ONE_CFG_PATH, JSON.stringify(cfg)); } catch { return Promise.resolve(false); }
  return new Promise((resolve) => {
    execFile(
      XRAY_EXE_PATH,
      ['-test', '-c', ONE_CFG_FILE],
      { cwd: DIR, windowsHide: true, timeout: 5000 },
      (err) => resolve(!err)
    );
  });
}

// links -> [{ link, name, protocol, address, port, ok, ms, ip }]
async function testLinks(links, onLog = () => {}) {
  const prepared = [];
  for (const link of links.slice(0, MAX_TEST)) {
    let parsed, cfg;
    try { ({ config: cfg, parsed } = generateLinkConfig(link)); } catch { continue; }
    if (!(await validateOne(link))) continue;
    prepared.push({ link, parsed, outbound: cfg.outbounds[0] });
  }
  if (!prepared.length) return [];

  const inbounds = [];
  const outbounds = [];
  const rules = [];
  prepared.forEach((p, i) => {
    const tag = 'out' + i;
    const ob = JSON.parse(JSON.stringify(p.outbound));
    ob.tag = tag;
    outbounds.push(ob);
    inbounds.push({ listen: '127.0.0.1', port: TEST_BASE_PORT + i, protocol: 'http', tag: 'in' + i });
    rules.push({ type: 'field', inboundTag: ['in' + i], outboundTag: tag });
  });
  const cfg = { log: { loglevel: 'none' }, inbounds, outbounds, routing: { rules } };
  fs.writeFileSync(POOL_CFG_PATH, JSON.stringify(cfg));

  const child = spawn(XRAY_EXE_PATH, ['-c', POOL_CFG_FILE], {
    cwd: DIR,
    windowsHide: true,
  });
  let errBuf = '';
  child.stderr.on('data', (d) => { errBuf += String(d); });
  child.stdout.on('data', (d) => { errBuf += String(d); });
  child.on('error', () => {});
  const ports = prepared.map((_, i) => TEST_BASE_PORT + i);
  const ready = await waitForPort(ports, 5000);
  if (!ready) onLog('тестовый xray не поднял порты: ' + (errBuf.trim().slice(0, 300) || 'без вывода'));
  const results = [];
  if (ready) {
    const probes = await Promise.all(ports.map((port) => probeViaPort(port)));
    prepared.forEach((p, i) => {
      results.push({
        link: p.link,
        name: p.parsed.name || p.parsed.address,
        protocol: p.parsed.protocol,
        address: p.parsed.address,
        port: p.parsed.port,
        ok: probes[i].ok,
        ms: probes[i].ms,
        ip: probes[i].ip || null,
      });
    });
  }
  try { child.kill(); } catch { /* ignore */ }
  try { fs.unlinkSync(POOL_CFG_PATH); } catch { /* ignore */ }
  return results;
}

module.exports = { safeFetchText, validateUrl, parseSubscriptionLinks, testLinks, isForbiddenIp };
