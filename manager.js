// Менеджер всего стека:
//   - запускает и следит за wireproxy (SOCKS5 25344 / HTTP 25345)
//   - умный HTTP-прокси на 127.0.0.1:8081: принимает весь браузерный трафик
//     и решает по доменам — через VPN или напрямую (без лимитов PAC)
//   - раздаёт PAC (крошечный) и веб-панель на http://127.0.0.1:8080/
//   - ведёт журнал «недавних сайтов мимо VPN» для апрува в панели
// Запуск: node manager.js (скрыто — через start-hidden.vbs)
'use strict';
const http = require('http');
const net = require('net');
const os = require('os');
const fs = require('fs');
const path = require('path');
const { spawn, execFile, execFileSync } = require('child_process');
const { updateDomains, normalizeDomain } = require('./update-domains');
const vlessXray = require('./vless-to-xray');

const IS_WIN = process.platform === 'win32';

const DIR = __dirname;
const PANEL_PORT = 8080;
const PROXY_PORT = 8081;
const HOST = '127.0.0.1';
const PAC_URL = `http://${HOST}:${PANEL_PORT}/proxy.pac`;
const UPSTREAM = { host: HOST, port: 25345 }; // HTTP-прокси wireproxy
const REG_KEY = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings';
const PROXY_OVERRIDE = 'localhost;127.0.0.1;*.local;192.168.*;10.*;172.16.*;<local>;zcode.z.ai;*.z.ai';
const EXTRA_FILE = path.join(DIR, 'extra-domains.txt');
const VLESS_DOMAINS_FILE = path.join(DIR, 'vless-domains.txt');
const SETTINGS_FILE = path.join(DIR, 'settings.json');
const UPSTREAM_WG = { host: HOST, port: 25345 };     // HTTP-прокси wireproxy (WireGuard)
const UPSTREAM_VLESS = { host: HOST, port: vlessXray.XRAY_HTTP_PORT }; // HTTP-прокси xray (VLESS)
const EXTRA_HEADER = [
  '# Домены, которые всегда должны идти через VPN (по одному в строке).',
  '# Строки с # — комментарии. Поддомены учитываются автоматически.',
  '# Файл редактируется и вручную, и через веб-панель http://127.0.0.1:8080/',
];
// Режим маршрутизации: 'blocked-only' — через VPN только домены из списков,
// 'all' — весь трафик через VPN.
const MODE = 'blocked-only';

const state = {
  wp: null,   // wireproxy child
  xr: null,   // xray child
  stopping: false,
  refreshing: false,
  proxiedSet: new Set(),
  directSet: new Set(),
  vlessSet: new Set(),
  vpnDefault: 'wireguard', // куда идут домены из списков: 'wireguard' | 'vless'
  proxied: null,
  direct: null,
  lastUpdate: null,
  updateError: null,
  exitIp: null,
  exitIpAt: 0,
  exitIpVless: null,
  exitIpVlessAt: 0,
  checks: [],
  visited: new Map(), // domain -> { count, last } — сайты, прошедшие мимо VPN
};

function log(...a) {
  console.log(new Date().toISOString(), ...a);
}

// ---------------- wireproxy ----------------
// Считаем WireGuard настроенным, только если wg.conf есть и в нём не заглушки
// из wg.conf.example. Иначе канал отключён (например, режим «только VLESS»).
const WG_CONF = path.join(DIR, 'wg.conf');
let wgCache = { mtime: 0, ok: false };

function isWgConfigured() {
  try {
    const st = fs.statSync(WG_CONF);
    if (st.mtimeMs !== wgCache.mtime) {
      const t = fs.readFileSync(WG_CONF, 'utf8');
      const ok =
        /^\s*PrivateKey\s*=\s*\S+/m.test(t) &&
        /^\s*Endpoint\s*=\s*\S+/m.test(t) &&
        !/ВАШ_|ПУБЛИЧНЫЙ_КЛЮЧ_СЕРВЕРА|vpn\.example\.com/i.test(t);
      wgCache = { mtime: st.mtimeMs, ok };
    }
    return wgCache.ok;
  } catch {
    return false;
  }
}

function startWireproxy() {
  if (state.stopping || state.wp || !isWgConfigured()) return;
  const child = spawn(path.join(DIR, IS_WIN ? 'wireproxy.exe' : 'wireproxy'), ['-c', 'wireproxy.conf'], {
    cwd: DIR,
    windowsHide: true,
  });
  state.wp = child;
  log('wireproxy запущен, pid', child.pid);
  child.stdout.on('data', () => {});
  child.stderr.on('data', () => {});
  child.on('exit', (code, signal) => {
    state.wp = null;
    if (state.stopping) return;
    log(`wireproxy завершился (code=${code}, signal=${signal}), перезапуск через 3 с`);
    setTimeout(startWireproxy, 3000);
  });
}

function stopWireproxy() {
  if (state.wp) {
    try { state.wp.kill(); } catch { /* ignore */ }
    state.wp = null;
  }
}

// ---------------- xray (VLESS) ----------------
function startXray() {
  if (state.stopping || state.xr || !vlessXray.hasConfig()) return;
  const child = spawn(path.join(DIR, IS_WIN ? 'xray.exe' : 'xray'), ['-c', 'xray-config.json'], {
    cwd: DIR,
    windowsHide: true,
  });
  state.xr = child;
  log('xray (VLESS) запущен, pid', child.pid);
  child.stdout.on('data', () => {});
  child.stderr.on('data', (d) => {
    const t = String(d).trim();
    if (t) log('xray:', t.slice(0, 200));
  });
  child.on('exit', (code, signal) => {
    state.xr = null;
    if (state.stopping || !vlessXray.hasConfig()) return;
    log(`xray завершился (code=${code}, signal=${signal}), перезапуск через 3 с`);
    setTimeout(startXray, 3000);
  });
}

function stopXray() {
  if (state.xr) {
    try { state.xr.kill(); } catch { /* ignore */ }
    state.xr = null;
  }
}

function restartXray() {
  stopXray();
  if (vlessXray.hasConfig()) setTimeout(startXray, 500);
}

// ---------------- настройки + vless-домены ----------------
function loadSettings() {
  try {
    const s = JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8'));
    if (s.vpnDefault === 'wireguard' || s.vpnDefault === 'vless') state.vpnDefault = s.vpnDefault;
  } catch { /* нет файла — значения по умолчанию */ }
}

function saveSettings() {
  fs.writeFileSync(SETTINGS_FILE, JSON.stringify({ vpnDefault: state.vpnDefault }, null, 2));
}

function readVlessDomains() {
  if (!fs.existsSync(VLESS_DOMAINS_FILE)) return [];
  return fs.readFileSync(VLESS_DOMAINS_FILE, 'utf8').split(/\r?\n/).map(normalizeDomain).filter(Boolean);
}

function writeVlessDomains(list) {
  const unique = Array.from(new Set(list)).sort();
  fs.writeFileSync(
    VLESS_DOMAINS_FILE,
    '# Домены, которые всегда должны идти через VLESS (по одному в строке).\n\n' +
    unique.join('\n') + (unique.length ? '\n' : '')
  );
  return unique;
}

// ---------------- вспомогательные ----------------
function run(cmd, args, cb) {
  execFile(cmd, args, { windowsHide: true, timeout: 30000 }, (e, stdout, stderr) => cb(e, stdout, stderr));
}

function portUp(port) {
  return new Promise((resolve) => {
    const s = net.connect({ host: '127.0.0.1', port });
    s.setTimeout(1000);
    s.on('connect', () => { s.destroy(); resolve(true); });
    s.on('error', () => resolve(false));
    s.on('timeout', () => { s.destroy(); resolve(false); });
  });
}

function checkExitIpVia(port) {
  return new Promise((resolve) => {
    run('curl', ['--http1.1', '-s', '-m', '12', '--proxy', `http://${HOST}:${port}`, 'https://api.ipify.org'],
      (e, stdout) => resolve(!e && stdout && stdout.trim() ? stdout.trim() : null));
  });
}

function checkExitIp() {
  return new Promise((resolve) => {
    if (Date.now() - state.exitIpAt < 60000) return resolve(state.exitIp);
    checkExitIpVia(UPSTREAM_WG.port).then((ip) => {
      if (ip) {
        state.exitIp = ip;
        state.exitIpAt = Date.now();
      }
      resolve(state.exitIp);
    });
  });
}

function checkExitIpVless() {
  return new Promise((resolve) => {
    if (Date.now() - state.exitIpVlessAt < 60000) return resolve(state.exitIpVless);
    checkExitIpVia(UPSTREAM_VLESS.port).then((ip) => {
      if (ip) {
        state.exitIpVless = ip;
        state.exitIpVlessAt = Date.now();
      }
      resolve(state.exitIpVless || null);
    });
  });
}

// ---------------- маршрутизация ----------------
// Возвращает false (напрямую) | 'wireguard' | 'vless'.
// Если предпочтительный канал не настроен — используется доступный.
function availableChannel(pref) {
  const wg = isWgConfigured();
  const xr = vlessXray.hasConfig();
  if (pref === 'vless') return xr ? 'vless' : (wg ? 'wireguard' : false);
  return wg ? 'wireguard' : (xr ? 'vless' : false);
}

function routeFor(hostRaw) {
  if (MODE === 'all') return availableChannel(state.vpnDefault);
  const host = String(hostRaw || '').toLowerCase().replace(/^\[|\]$/g, '');
  if (!host || !host.includes('.') || net.isIP(host)) return false;
  for (let h = host; h; ) {
    if (state.directSet.has(h)) return false;
    if (state.vlessSet.has(h)) return availableChannel('vless');
    if (state.proxiedSet.has(h)) return availableChannel(state.vpnDefault);
    const i = h.indexOf('.');
    if (i < 0) break;
    h = h.slice(i + 1);
  }
  return false;
}

// Канал по умолчанию, но с учётом того, какой процесс реально запущен
function activeChannel(route) {
  if (!route) return false;
  if (route === 'vless') return state.xr ? 'vless' : (state.wp ? 'wireguard' : 'vless');
  return state.wp ? 'wireguard' : (state.xr ? 'vless' : 'wireguard');
}

function shouldProxy(hostRaw) {
  return !!routeFor(hostRaw);
}

// Журнал «недавних сайтов мимо VPN» — кандидаты на добавление в панели
function noteVisited(hostRaw) {
  const host = String(hostRaw || '').toLowerCase().replace(/^\[|\]$/g, '').split(':')[0];
  if (!host || !host.includes('.') || net.isIP(host)) return;
  if (shouldProxy(host)) return; // интересуют только те, что идут напрямую
  const now = Date.now();
  const rec = state.visited.get(host);
  if (rec) {
    rec.count++;
    rec.last = now;
    state.visited.delete(host);
    state.visited.set(host, rec);
  } else {
    state.visited.set(host, { count: 1, last: now });
    if (state.visited.size > 300) {
      state.visited.delete(state.visited.keys().next().value);
    }
  }
}

// ---------------- системный прокси (статический, без PAC-лимитов) ----------------
// Windows: реестр WinINET + refresh через InternetSetOption.
// macOS: networksetup для активного сетевого сервиса (web + secure web proxy).
const PROXY_BYPASS_DOMAINS = ['localhost', '127.0.0.1', '*.local', '192.168.*', '10.*', '172.16.*', 'zcode.z.ai', '*.z.ai'];

function refreshWininet() {
  if (!IS_WIN) return;
  const ps =
    "Add-Type -Namespace Win -Name Inet -MemberDefinition '[DllImport(\"wininet.dll\")] public static extern bool InternetSetOption(IntPtr h,int o,IntPtr b,int l);'; " +
    '[Win.Inet]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0); ' +
    '[Win.Inet]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0);';
  run('powershell', ['-NoProfile', '-Command', ps], () => {});
}

// Активный сетевой сервис macOS (по интерфейсу default-маршрута, напр. Wi-Fi)
function macPrimaryService() {
  return new Promise((resolve) => {
    execFile('bash', ['-c', "route -n get default 2>/dev/null | awk '/interface:/{print $2}'"], (e, so) => {
      const dev = String(so || '').trim();
      if (e || !dev) return resolve(null);
      execFile('networksetup', ['-listallhardwareports'], (e2, so2) => {
        if (e2) return resolve(null);
        for (const block of String(so2 || '').split(/(?=Hardware Port:)/)) {
          const m = block.match(/Hardware Port:\s*(.+)\r?\nDevice:\s*(\S+)/);
          if (m && m[2] === dev) return resolve(m[1].trim());
        }
        resolve(null);
      });
    });
  });
}

function nsGet(args) {
  return new Promise((resolve) => execFile('networksetup', args, { timeout: 15000 }, (e, stdout) => resolve(String(stdout || ''))));
}

function nsRun(args) {
  return new Promise((resolve) => execFile('networksetup', args, { timeout: 15000 }, () => resolve()));
}

function nsParseProxy(text) {
  return {
    enabled: /Enabled:\s*Yes/i.test(text),
    server: (/Server:\s*(\S+)/.exec(text) || [])[1],
    port: (/Port:\s*(\d+)/.exec(text) || [])[1],
    url: (/URL:\s*(\S+)/.exec(text) || [])[1],
  };
}

async function getSystemProxyMac(svc) {
  const [web, sec, auto] = await Promise.all([
    nsGet(['-getwebproxy', svc]),
    nsGet(['-getsecurewebproxy', svc]),
    nsGet(['-getautoproxyurl', svc]),
  ]);
  const w = nsParseProxy(web);
  const s = nsParseProxy(sec);
  const a = nsParseProxy(auto);
  if (w.enabled && s.enabled && w.server === HOST && String(w.port) === String(PROXY_PORT)) {
    return { enabled: true, mode: 'static', value: `${HOST}:${PROXY_PORT}` };
  }
  if (a.enabled && a.url) return { enabled: false, mode: 'pac', value: a.url };
  return { enabled: false, mode: null, value: null };
}

async function setSystemProxyMac(svc, enable) {
  if (enable) {
    await nsRun(['-setautoproxystate', svc, 'off']); // старый PAC долой
    await nsRun(['-setwebproxy', svc, HOST, String(PROXY_PORT)]);
    await nsRun(['-setsecurewebproxy', svc, HOST, String(PROXY_PORT)]);
    await nsRun(['-setproxybypassdomains', svc, ...PROXY_BYPASS_DOMAINS]);
  } else {
    await nsRun(['-setwebproxystate', svc, 'off']);
    await nsRun(['-setsecurewebproxystate', svc, 'off']);
  }
  return getSystemProxyMac(svc);
}

async function getSystemProxy() {
  if (!IS_WIN) {
    const svc = await macPrimaryService();
    if (!svc) return { enabled: false, mode: null, value: null, service: null };
    return { ...(await getSystemProxyMac(svc)), service: svc };
  }
  return new Promise((resolve) => {
    run('reg', ['query', REG_KEY], (e, stdout) => {
      if (e) return resolve({ enabled: false, mode: null, value: null });
      const out = String(stdout);
      const enabled = /ProxyEnable\s+REG_DWORD\s+0x1/.test(out);
      const ps = /ProxyServer\s+REG_SZ\s+(\S+)/.exec(out);
      const ac = /AutoConfigURL\s+REG_SZ\s+(\S+)/.exec(out);
      if (enabled && ps && ps[1] === `${HOST}:${PROXY_PORT}`) {
        return resolve({ enabled: true, mode: 'static', value: ps[1] });
      }
      if (ac) return resolve({ enabled: false, mode: 'pac', value: ac[1] });
      resolve({ enabled: false, mode: null, value: null });
    });
  });
}

async function setSystemProxy(enable) {
  if (!IS_WIN) {
    const svc = await macPrimaryService();
    if (!svc) throw new Error('не найден активный сетевой сервис (Wi-Fi/Ethernet)');
    return setSystemProxyMac(svc, enable);
  }
  const steps = enable
    ? [
        ['add', REG_KEY, '/v', 'ProxyEnable', '/t', 'REG_DWORD', '/d', '1', '/f'],
        ['add', REG_KEY, '/v', 'ProxyServer', '/t', 'REG_SZ', '/d', `${HOST}:${PROXY_PORT}`, '/f'],
        ['add', REG_KEY, '/v', 'ProxyOverride', '/t', 'REG_SZ', '/d', PROXY_OVERRIDE, '/f'],
        ['delete', REG_KEY, '/v', 'AutoConfigURL', '/f'],
      ]
    : [['add', REG_KEY, '/v', 'ProxyEnable', '/t', 'REG_DWORD', '/d', '0', '/f']];
  for (const args of steps) {
    await new Promise((r) => run('reg', args, () => r())); // ошибки (например, нет AutoConfigURL) игнорируем
  }
  refreshWininet();
  return getSystemProxy();
}

// Синхронно выключает системный прокси — вызывается при выходе менеджера,
// чтобы остановленный стек не оставлял браузер с ERR_PROXY_CONNECTION_FAILED.
function disableSystemProxySync() {
  if (!IS_WIN) return;
  try {
    execFileSync('reg', ['add', REG_KEY, '/v', 'ProxyEnable', '/t', 'REG_DWORD', '/d', '0', '/f'], {
      windowsHide: true,
      timeout: 5000,
    });
  } catch { /* ignore */ }
}

// ---------------- проверка домена: напрямую vs через VPN ----------------
// Статусы HTTPS-ответов внутри TLS не видны, поэтому «автосбор 403»
// невозможен без MITM. Вместо этого панель проверяет домен по запросу.
function httpStatus(domain, viaProxy) {
  return new Promise((resolve) => {
    const args = ['--http1.1', '-s', '-o', os.devNull, '-m', '12', '-w', '%{http_code}', 'https://' + domain + '/'];
    if (viaProxy) {
      // проверяем тот канал, который сейчас реально обслуживает VPN-домены
      const chan = activeChannel(state.vpnDefault) || 'wireguard';
      const port = chan === 'vless' ? UPSTREAM_VLESS.port : UPSTREAM_WG.port;
      args.unshift('--proxy', `http://${HOST}:${port}`);
    }
    run('curl', args, (e, stdout) => {
      const code = e ? '000' : String(stdout || '').trim();
      resolve(/^\d{3}$/.test(code) ? code : '000');
    });
  });
}

const PROXY_OK = (code) => code !== '000' && code !== '403' && code !== '451';

function verdict(direct, vpn) {
  if (PROXY_OK(vpn) && !PROXY_OK(direct)) return 'vpn-helps';
  if (PROXY_OK(direct) && !PROXY_OK(vpn)) return 'direct-only';
  if (PROXY_OK(direct) && PROXY_OK(vpn)) return 'both-ok';
  if (vpn === '000') return 'blocked-everywhere';
  // 403/451 на обоих путях — обычно анти-бот проверка (Cloudflare), которую
  // curl не проходит, а браузер проходит. В браузере через VPN часто работает.
  return 'challenge';
}

async function checkDomain(domain) {
  const [direct, vpn] = await Promise.all([httpStatus(domain, false), httpStatus(domain, true)]);
  const entry = { domain, direct, vpn, verdict: verdict(direct, vpn), at: new Date().toISOString() };
  state.checks.unshift(entry);
  if (state.checks.length > 25) state.checks.length = 25;
  log(`проверка ${domain}: напрямую ${direct}, через VPN ${vpn} -> ${entry.verdict}`);
  return entry;
}

// ---------------- extra-domains ----------------
function readExtra() {
  if (!fs.existsSync(EXTRA_FILE)) return [];
  return fs.readFileSync(EXTRA_FILE, 'utf8').split(/\r?\n/).map(normalizeDomain).filter(Boolean);
}

function writeExtra(list) {
  const unique = Array.from(new Set(list)).sort();
  fs.writeFileSync(EXTRA_FILE, EXTRA_HEADER.join('\n') + '\n\n' + unique.join('\n') + (unique.length ? '\n' : ''));
  return unique;
}

// ---------------- обновление списков ----------------
function doRefresh() {
  if (state.refreshing) return Promise.resolve({ already: true });
  state.refreshing = true;
  return updateDomains()
    .then((r) => {
      state.proxiedSet = new Set(r.proxied);
      state.directSet = new Set(r.direct);
      state.proxied = r.proxied.length;
      state.direct = r.direct.length;
      state.lastUpdate = new Date().toISOString();
      state.updateError = null;
      log(`список обновлён: ${r.proxied.length} через VPN, ${r.direct.length} исключений`);
    })
    .catch((e) => {
      state.updateError = e.message;
      log('ошибка обновления списка:', e.message);
    })
    .finally(() => { state.refreshing = false; });
}

// ---------------- умный HTTP-прокси (8081) ----------------
function pipeSockets(a, b, head) {
  if (head && head.length) b.write(head);
  a.pipe(b);
  b.pipe(a);
  const kill = () => { try { a.destroy(); } catch {} try { b.destroy(); } catch {} };
  a.on('error', kill);
  b.on('error', kill);
  a.on('close', () => b.destroy());
  b.on('close', () => a.destroy());
}

function parseHostPort(url, defPort) {
  const i = url.lastIndexOf(':');
  if (i < 0) return { host: url.replace(/^\[|\]$/g, ''), port: defPort };
  return { host: url.slice(0, i).replace(/^\[|\]$/g, ''), port: Number(url.slice(i + 1)) || defPort };
}

const proxyServer = http.createServer((req, res) => {
  // Обычный HTTP-прокси запрос: абсолютный URI (GET http://example.com/ ...)
  if (!/^https?:\/\//i.test(req.url)) {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    return res.end('это прокси-порт; панель на ' + PANEL_PORT);
  }
  let u;
  try { u = new URL(req.url); } catch { res.writeHead(400); return res.end(); }
  noteVisited(u.hostname);
  const route = routeFor(u.hostname);
  const chan = activeChannel(route);
  log(`${req.method} ${u.hostname}${chan ? ' [VPN:' + chan + ']' : ' [direct]'}`);
  const headers = { ...req.headers };
  delete headers.proxy_connection;
  delete headers['proxy-connection'];
  const up = chan
    ? (() => {
        const target = chan === 'vless' ? UPSTREAM_VLESS : UPSTREAM_WG;
        return http.request({ host: target.host, port: target.port, method: req.method, path: req.url, headers }); // прокси сам делает CONNECT
      })()
    : http.request({ host: u.hostname, port: u.port || 80, method: req.method, path: u.pathname + u.search, headers });
  const upstream = up;
  upstream.on('error', () => { try { res.writeHead(502); res.end('upstream error'); } catch { /* ignore */ } });
  req.pipe(upstream);
});

proxyServer.on('connect', (req, clientSocket, head) => {
  const { host, port } = parseHostPort(req.url, 443);
  noteVisited(host);
  const route = routeFor(host);
  const chan = activeChannel(route);
  const target = chan === 'vless' ? UPSTREAM_VLESS : UPSTREAM_WG;
  log(`CONNECT ${host}:${port}${chan ? ' [VPN:' + chan + ']' : ' [direct]'}`);
  clientSocket.on('error', () => {});

  if (chan) {
    // Цепочка: браузер -> менеджер -> (wireproxy | xray) -> туннель
    const up = http.request({ host: target.host, port: target.port, method: 'CONNECT', path: req.url });
    up.on('connect', (ures, upSocket) => {
      if (ures.statusCode !== 200) {
        log(`upstream отказал для ${host}: ${ures.statusCode}`);
        clientSocket.destroy();
        upSocket.destroy();
        return;
      }
      clientSocket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      pipeSockets(clientSocket, upSocket, head);
    });
    up.on('error', (e) => { log(`upstream error ${host}: ${e.message}`); clientSocket.destroy(); });
    up.end();
  } else {
    const up = net.connect(port, host, () => {
      // реальный IP сервера, к которому ушло прямое соединение (в DevTools он не виден)
      log(`  -> ${host}:${port} = ${up.remoteAddress} [direct]`);
      clientSocket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
      pipeSockets(clientSocket, up, head);
    });
    up.on('error', () => clientSocket.destroy());
    clientSocket.on('error', () => up.destroy());
  }
});

// ---------------- панель (8080) ----------------
function sendJson(res, code, obj) {
  res.writeHead(code, { 'Content-Type': 'application/json; charset=utf-8' });
  res.end(JSON.stringify(obj));
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let data = '';
    req.on('data', (c) => { data += c; if (data.length > 1e6) req.destroy(); });
    req.on('end', () => {
      try { resolve(data ? JSON.parse(data) : {}); } catch (e) { reject(e); }
    });
    req.on('error', reject);
  });
}

const panel = http.createServer(async (req, res) => {
  const url = (req.url || '/').split('?')[0];
  try {
    if (req.method === 'GET' && (url === '/' || url === '/index.html')) {
      const html = fs.readFileSync(path.join(DIR, 'ui.html'));
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-cache' });
      return res.end(html);
    }
    if (req.method === 'GET' && url === '/proxy.pac') {
      const pac = fs.readFileSync(path.join(DIR, 'proxy.pac'));
      res.writeHead(200, {
        'Content-Type': 'application/x-ns-proxy-autoconfig',
        'Content-Length': pac.length,
        'Cache-Control': 'no-cache',
      });
      return res.end(pac);
    }
    if (req.method === 'GET' && url === '/api/status') {
      const [vpnUp, sysProxy, exitIp, xrayPortUp, exitIpVless] = await Promise.all([
        portUp(UPSTREAM_WG.port),
        getSystemProxy(),
        checkExitIp(),
        portUp(UPSTREAM_VLESS.port),
        checkExitIpVless(),
      ]);
      return sendJson(res, 200, {
        vpnUp,
        wgConfigured: isWgConfigured(),
        proxyRunning: !!state.wp,
        exitIp,
        xrayRunning: !!state.xr,
        vlessPortUp: xrayPortUp,
        vlessConfigured: vlessXray.hasConfig(),
        exitIpVless: state.xr ? exitIpVless : null,
        vpnDefault: state.vpnDefault,
        vlessDomains: readVlessDomains(),
        proxied: state.proxied,
        direct: state.direct,
        lastUpdate: state.lastUpdate,
        updateError: state.updateError,
        refreshing: state.refreshing,
        mode: MODE,
        systemProxy: sysProxy,
        extra: readExtra(),
        checks: state.checks,
        visited: Array.from(state.visited, ([domain, v]) => ({ domain, count: v.count, last: v.last }))
          .sort((a, b) => b.last - a.last)
          .slice(0, 60),
      });
    }
    if (req.method === 'POST' && url === '/api/extra') {
      const body = await readBody(req);
      const d = normalizeDomain(String(body.domain || ''));
      if (!d) return sendJson(res, 400, { error: 'некорректный домен' });
      let list = readExtra();
      if (body.action === 'add') {
        if (!list.includes(d)) list.push(d);
      } else if (body.action === 'remove') {
        list = list.filter((x) => x !== d);
        state.visited.delete(d);
      } else {
        return sendJson(res, 400, { error: 'action должен быть add|remove' });
      }
      writeExtra(list);
      doRefresh(); // перегенерирует списки в памяти менеджера (быстро, PAC уже маленький)
      return sendJson(res, 200, { ok: true, extra: readExtra() });
    }
    if (req.method === 'POST' && url === '/api/vless') {
      const body = await readBody(req);
      if (body.action === 'set-link') {
        try {
          const info = vlessXray.saveLink(String(body.link || ''));
          log('VLESS-конфиг сохранён:', JSON.stringify(info));
          restartXray();
          return sendJson(res, 200, { ok: true, info });
        } catch (e) {
          return sendJson(res, 400, { error: e.message });
        }
      }
      if (body.action === 'remove-link') {
        if (fs.existsSync(path.join(DIR, 'xray-config.json'))) fs.unlinkSync(path.join(DIR, 'xray-config.json'));
        stopXray();
        return sendJson(res, 200, { ok: true });
      }
      if (body.action === 'set-default') {
        if (body.value !== 'wireguard' && body.value !== 'vless') {
          return sendJson(res, 400, { error: 'value должен быть wireguard|vless' });
        }
        state.vpnDefault = body.value;
        saveSettings();
        log('основной VPN-канал:', body.value);
        return sendJson(res, 200, { ok: true, vpnDefault: state.vpnDefault });
      }
      if (body.action === 'add-domain' || body.action === 'remove-domain') {
        const d = normalizeDomain(String(body.domain || ''));
        if (!d) return sendJson(res, 400, { error: 'некорректный домен' });
        let list = readVlessDomains();
        if (body.action === 'add-domain') {
          if (!list.includes(d)) list.push(d);
        } else {
          list = list.filter((x) => x !== d);
        }
        writeVlessDomains(list);
        state.vlessSet = new Set(readVlessDomains());
        return sendJson(res, 200, { ok: true, vlessDomains: list });
      }
      if (body.action === 'restart') {
        restartXray();
        return sendJson(res, 200, { ok: true });
      }
      return sendJson(res, 400, { error: 'неизвестное действие' });
    }
    if (req.method === 'POST' && url === '/api/check') {
      const body = await readBody(req);
      const d = normalizeDomain(String(body.domain || ''));
      if (!d) return sendJson(res, 400, { error: 'некорректный домен' });
      const entry = await checkDomain(d);
      return sendJson(res, 200, entry);
    }
    if (req.method === 'POST' && url === '/api/refresh') {
      doRefresh();
      return sendJson(res, 202, { ok: true });
    }
    if (req.method === 'POST' && url === '/api/restart-proxy') {
      log('перезапуск wireproxy по запросу из панели');
      stopWireproxy();
      setTimeout(startWireproxy, 1000);
      return sendJson(res, 200, { ok: true });
    }
    if (req.method === 'POST' && url === '/api/system-proxy') {
      const body = await readBody(req);
      const st = await setSystemProxy(!!body.enabled);
      return sendJson(res, 200, { ok: true, state: st });
    }
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('not found');
  } catch (e) {
    sendJson(res, 500, { error: e.message });
  }
});

panel.listen(PANEL_PORT, HOST, () => {
  log(`панель: http://${HOST}:${PANEL_PORT}/  |  PAC: ${PAC_URL}`);
  loadSettings();
  state.vlessSet = new Set(readVlessDomains());
  if (!isWgConfigured()) log('WireGuard не настроен (нет корректного wg.conf) — канал отключён');
  if (!vlessXray.hasConfig()) log('VLESS не настроен (нет vless:// ссылки) — канал отключён');
  startWireproxy();
  startXray();
  doRefresh();
  setInterval(doRefresh, 12 * 60 * 60 * 1000);
  setInterval(() => checkExitIp(), 60 * 1000);
  // миграция: если раньше был включён системный PAC — переключаем на статический прокси
  getSystemProxy().then((st) => {
    if (st.mode === 'pac' && st.value === PAC_URL) {
      log('миграция: системный PAC -> статический прокси 8081');
      setSystemProxy(true);
    }
  });
});

proxyServer.listen(PROXY_PORT, HOST, () => {
  log(`умный прокси: ${HOST}:${PROXY_PORT} (режим ${MODE})`);
});

process.on('exit', () => { state.stopping = true; stopWireproxy(); stopXray(); disableSystemProxySync(); });
process.on('SIGINT', () => process.exit(0));
process.on('SIGTERM', () => process.exit(0));
