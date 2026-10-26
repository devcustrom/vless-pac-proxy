// Парсит vless:// ссылку и генерирует конфиг xray-core.
// Поддержка: Reality, TLS, plain; сети tcp / ws / grpc; flow xtls-rprx-vision.
// Использование: node vless-to-xray.js <vless-url>   (пишет xray-config.json)
// Из manager.js вызывается generateXrayConfig(link).
'use strict';
const fs = require('fs');
const path = require('path');

const DIR = __dirname;
const XRAY_CONFIG = path.join(DIR, 'xray-config.json');
const XRAY_HTTP_PORT = 25346; // локальный HTTP-прокси xray, через него идёт «VLESS-канал»

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
  const id = s.slice(0, at);
  const hostport = s.slice(at + 1);
  const ci = hostport.lastIndexOf(':');
  if (ci < 0) throw new Error('нет порта в vless-ссылке');
  const address = hostport.slice(0, ci).replace(/^\[|\]$/g, '');
  const port = Number(hostport.slice(ci + 1));
  if (!id || !address || !port) throw new Error('не удалось разобрать uuid/адрес/порт');

  const p = (k, d) => (params.get(k) || d || '').toLowerCase();
  const stream = {
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
  };
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id)) {
    throw new Error('uuid в ссылке выглядит некорректно');
  }
  return { id, address, port, name, stream };
}

function generateXrayConfig(raw) {
  const v = parseVlessUrl(raw);
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

  const config = {
    log: { loglevel: 'warning' },
    inbounds: [
      { listen: '127.0.0.1', port: XRAY_HTTP_PORT, protocol: 'http', settings: { allowTransparent: false } },
    ],
    outbounds: [
      {
        tag: 'vless-out',
        protocol: 'vless',
        settings: { vnext: [{ address: v.address, port: v.port, users: [user] }] },
        streamSettings,
        mux: { enabled: false, concurrency: -1 },
      },
    ],
  };
  return { config, parsed: v };
}

// Записывает конфиг, если ссылка валидна. Возвращает краткое описание.
// Прежний конфиг сохраняется рядом (xray-config.json.bak) — на случай ошибки.
function saveLink(raw) {
  const { config, parsed } = generateXrayConfig(raw);
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
    network: parsed.stream.network,
    security: parsed.stream.security,
  };
}

function hasConfig() {
  return fs.existsSync(XRAY_CONFIG);
}

module.exports = { saveLink, hasConfig, generateXrayConfig, XRAY_HTTP_PORT };

if (require.main === module) {
  try {
    const info = saveLink(process.argv[2] || '');
    console.log('xray-config.json записан:', JSON.stringify(info));
  } catch (e) {
    console.error('ОШИБКА:', e.message);
    process.exit(1);
  }
}
