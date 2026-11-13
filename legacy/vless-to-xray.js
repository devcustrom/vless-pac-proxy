// Парсит vless:// и ss:// ссылки и генерирует конфиг xray-core.
// VLESS: Reality, TLS, plain; сети tcp / ws / grpc; flow xtls-rprx-vision.
// SS: SIP002 (ss://base64(method:password)@host:port#name и legacy вариант).
// Использование: node vless-to-xray.js <link>   (пишет xray-config.json)
// Из manager.js вызывается generateLinkConfig(link) / saveLink(link).
'use strict';
const fs = require('fs');
const path = require('path');

const DIR = __dirname;
const XRAY_CONFIG = path.join(DIR, 'xray-config.json');
const XRAY_HTTP_PORT = 25346; // локальный HTTP-прокси xray, через него идёт «VLESS-канал»

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function validateUuid(id) {
  if (!UUID_RE.test(id)) throw new Error('uuid в ссылке выглядит некорректно');
  return id;
}

function parseVlessUrl(raw) {
  let s = String(raw || '').trim();
  if (!s.toLowerCase().startsWith('vless://')) {
    throw new Error('ссылка должна начинаться с vless://');
  }
  s = s.slice('vless://'.length);
  const hash = s.indexOf('#');
  let name = '';
  if (hash >= 0) {
    try { name = decodeURIComponent(s.slice(hash + 1)); } catch { name = s.slice(hash + 1); }
    s = s.slice(0, hash);
  }
  const q = s.indexOf('?');
  let params = new URLSearchParams();
  if (q >= 0) {
    params = new URLSearchParams(s.slice(q + 1));
    s = s.slice(0, q);
  }
  const at = s.lastIndexOf('@');
  if (at < 0) throw new Error('нет @ в vless-ссылке (ожидается vless://uuid@host:port)');
  const id = validateUuid(s.slice(0, at));
  const hostport = s.slice(at + 1);
  const ci = hostport.lastIndexOf(':');
  if (ci < 0) throw new Error('нет порта в vless-ссылке');
  const address = hostport.slice(0, ci).replace(/^\[|\]$/g, '');
  const port = Number(hostport.slice(ci + 1));
  if (!id || !address || !port) throw new Error('не удалось разобрать uuid/адрес/порт');

  const p = (k, d) => (params.get(k) || d || '').toLowerCase();
  return {
    protocol: 'vless',
    id,
    address,
    port,
    name,
    stream: {
      network: p('type', 'tcp'),
      security: p('security', 'none'),
      fingerprint: p('fp', '') || undefined,
      sni: params.get('sni') || params.get('host') || undefined,
      publicKey: params.get('pbk') || undefined,
      shortId: params.get('sid') || undefined,
      flow: params.get('flow') || undefined,
      path: params.get('path') || undefined,
      serviceName: params.get('serviceName') || undefined,
      hostHeader: params.get('host') || undefined,
    },
  };
}

function parseSsUrl(raw) {
  let s = String(raw || '').trim();
  if (!s.toLowerCase().startsWith('ss://')) throw new Error('ссылка должна начинаться с ss://');
  s = s.slice('ss://'.length);
  const hash = s.indexOf('#');
  let name = '';
  if (hash >= 0) {
    try { name = decodeURIComponent(s.slice(hash + 1)); } catch { name = s.slice(hash + 1); }
    s = s.slice(0, hash);
  }
  const q = s.indexOf('?'); // plugin и пр. игнорируем
  if (q >= 0) s = s.slice(0, q);

  let method, password, address, port;
  const at = s.lastIndexOf('@');
  if (at >= 0) {
    // SIP002: ss://base64(method:password)@host:port
    const userinfo = s.slice(0, at);
    const hostport = s.slice(at + 1);
    let decoded;
    try { decoded = Buffer.from(userinfo, 'base64').toString('utf8'); } catch { decoded = ''; }
    if (!decoded.includes(':')) throw new Error('не удалось разобрать ss userinfo');
    [method, password] = decoded.split(':', 2);
    const ci = hostport.lastIndexOf(':');
    if (ci < 0) throw new Error('нет порта в ss-ссылке');
    address = hostport.slice(0, ci).replace(/^\[|\]$/g, '');
    port = Number(hostport.slice(ci + 1));
  } else {
    // legacy: ss://base64(method:password@host:port)
    let decoded;
    try { decoded = Buffer.from(s, 'base64').toString('utf8'); } catch { decoded = ''; }
    const m = /^(.+?):(.*?)@(.+):(\d+)$/.exec(decoded);
    if (!m) throw new Error('не удалось разобрать legacy ss-ссылку');
    method = m[1];
    password = m[2];
    address = m[3];
    port = Number(m[4]);
  }
  if (!method || !password || !address || !port) throw new Error('не удалось разобрать ss-ссылку');
  return {
    protocol: 'ss',
    address,
    port,
    name,
    method: method.toLowerCase(),
    password,
    stream: { network: 'tcp', security: 'none' },
  };
}

function parseAnyLink(raw) {
  const s = String(raw || '').trim();
  const lower = s.toLowerCase();
  if (lower.startsWith('vless://')) return parseVlessUrl(s);
  if (lower.startsWith('ss://')) return parseSsUrl(s);
  throw new Error('поддерживаются только vless:// и ss:// ссылки');
}

function buildOutbound(v) {
  if (v.protocol === 'vless') {
    const user = { id: v.id, encryption: 'none', level: 0 };
    if (v.stream.security === 'reality' && v.stream.flow) user.flow = v.stream.flow;

    const streamSettings = { network: v.stream.network, security: v.stream.security };
    if (v.stream.security === 'reality') {
      streamSettings.realitySettings = {
        serverName: v.stream.sni,
        fingerprint: v.stream.fingerprint || 'chrome',
        publicKey: v.stream.publicKey,
        shortId: v.stream.shortId || '',
        spiderX: '/',
      };
    } else if (v.stream.security === 'tls') {
      streamSettings.tlsSettings = {
        serverName: v.stream.sni,
        fingerprint: v.stream.fingerprint || 'chrome',
        allowInsecure: false,
      };
    }
    if (v.stream.network === 'ws') {
      streamSettings.wsSettings = {
        path: v.stream.path || '/',
        headers: v.stream.hostHeader ? { Host: v.stream.hostHeader } : undefined,
      };
    } else if (v.stream.network === 'grpc') {
      streamSettings.grpcSettings = { serviceName: v.stream.serviceName || '' };
    }
    return {
      tag: 'vless-out',
      protocol: 'vless',
      settings: { vnext: [{ address: v.address, port: v.port, users: [user] }] },
      streamSettings,
      mux: { enabled: false, concurrency: -1 },
    };
  }
  // shadowsocks
  return {
    tag: 'ss-out',
    protocol: 'shadowsocks',
    settings: {
      servers: [{ address: v.address, port: v.port, method: v.method, password: v.password }],
    },
    mux: { enabled: false, concurrency: -1 },
  };
}

function generateLinkConfig(raw) {
  const v = parseAnyLink(raw);
  const config = {
    log: { loglevel: 'warning' },
    inbounds: [
      { listen: '127.0.0.1', port: XRAY_HTTP_PORT, protocol: 'http', settings: { allowTransparent: false } },
    ],
    outbounds: [buildOutbound(v)],
  };
  return { config, parsed: v };
}

const generateXrayConfig = generateLinkConfig; // обратная совместимость

// Записывает конфиг, если ссылка валидна. Возвращает краткое описание.
// Прежний конфиг сохраняется рядом (xray-config.json.bak) — на случай ошибки.
function saveLink(raw) {
  const { config, parsed } = generateLinkConfig(raw);
  if (fs.existsSync(XRAY_CONFIG)) {
    try { fs.copyFileSync(XRAY_CONFIG, XRAY_CONFIG + '.bak'); } catch { /* ignore */ }
  }
  const tmp = XRAY_CONFIG + '.tmp';
  fs.writeFileSync(tmp, JSON.stringify(config, null, 2));
  fs.renameSync(tmp, XRAY_CONFIG);
  return {
    name: parsed.name || parsed.address,
    address: parsed.address,
    port: parsed.port,
    protocol: parsed.protocol,
    network: parsed.stream.network,
    security: parsed.stream.security,
  };
}

function hasConfig() {
  return fs.existsSync(XRAY_CONFIG);
}

module.exports = { saveLink, hasConfig, generateLinkConfig, generateXrayConfig, parseAnyLink, XRAY_HTTP_PORT };

if (require.main === module) {
  try {
    const info = saveLink(process.argv[2] || '');
    console.log('xray-config.json записан:', JSON.stringify(info));
  } catch (e) {
    console.error('ОШИБКА:', e.message);
    process.exit(1);
  }
}
