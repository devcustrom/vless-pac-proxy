// Локальный CA и сертификаты для HTTPS у плагина локальных доменов.
// Ключи и сертификаты лежат в ./certs (в .gitignore). Никаких внешних запросов:
// всё генерируется локально через openssl (вызов без shell, массив аргументов).
'use strict';
const fs = require('fs');
const path = require('path');
const { execFile, execFileSync } = require('child_process');

const DIR = __dirname;
const CERTS_DIR = path.join(DIR, 'certs');
const DOMAIN_RE = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)*$/;

// Имена файлов — только basename внутри CERTS_DIR (без выхода за границы).
function certPath(name) {
  if (typeof name !== 'string' || path.basename(name) !== name || name === '.' || name === '..') {
    throw new Error('некорректное имя файла сертификата');
  }
  const base = path.resolve(CERTS_DIR);
  const p = path.resolve(base, name);
  if (!p.startsWith(base + path.sep)) throw new Error('путь вне каталога сертификатов');
  return p;
}

function findOpenssl() {
  const candidates = [
    process.env.OPENSSL,
    'openssl',
    'C:/Program Files/Git/usr/bin/openssl.exe',
    'C:/Program Files/Git/mingw64/bin/openssl.exe',
    '/usr/bin/openssl',
    '/usr/local/bin/openssl',
    '/opt/homebrew/bin/openssl',
  ].filter(Boolean);
  for (const bin of candidates) {
    try {
      execFileSync(bin, ['version'], { stdio: 'ignore', timeout: 5000 });
      return bin;
    } catch { /* пробуем следующий */ }
  }
  return null;
}

function run(openssl, args) {
  return new Promise((resolve, reject) => {
    execFile(openssl, args, { timeout: 30000 }, (err, stdout, stderr) => {
      if (err) return reject(new Error(String(stderr || err.message).trim().slice(0, 300)));
      resolve(String(stdout));
    });
  });
}

// CA создаётся один раз: certs/ca.key + certs/ca.crt (10 лет)
async function ensureCa(openssl) {
  fs.mkdirSync(CERTS_DIR, { recursive: true });
  const caKey = certPath('ca.key');
  const caCrt = certPath('ca.crt');
  if (fs.existsSync(caKey) && fs.existsSync(caCrt)) return { caKey, caCrt };
  await run(openssl, ['genrsa', '-out', caKey, '2048']);
  await run(openssl, [
    'req', '-new', '-x509', '-days', '3650',
    '-key', caKey, '-out', caCrt,
    '-subj', '/CN=wireproxy-control local CA',
    '-addext', 'basicConstraints=critical,CA:TRUE',
    '-addext', 'keyUsage=critical,keyCertSign,cRLSign',
  ]);
  return { caKey, caCrt };
}

// Сертификат для домена, подписанный локальным CA (825 дней, SAN=DNS:домен)
async function ensureDomainCert(openssl, ca, domain) {
  const d = String(domain || '').toLowerCase();
  if (!DOMAIN_RE.test(d) || d.length > 253) throw new Error('некорректный домен для сертификата');
  fs.mkdirSync(CERTS_DIR, { recursive: true });
  const key = certPath(d + '.key');
  const crt = certPath(d + '.crt');
  const csr = certPath(d + '.csr');
  if (fs.existsSync(key) && fs.existsSync(crt)) return { key, crt };
  await run(openssl, ['genrsa', '-out', key, '2048']);
  await run(openssl, [
    'req', '-new', '-key', key, '-out', csr,
    '-subj', '/CN=' + d,
    '-addext', 'subjectAltName=DNS:' + d,
  ]);
  await run(openssl, [
    'x509', '-req', '-in', csr,
    '-CA', ca.caCrt, '-CAkey', ca.caKey, '-CAcreateserial',
    '-days', '825', '-out', crt,
    '-copy_extensions', 'copy',
  ]);
  try { fs.unlinkSync(csr); } catch { /* ignore */ }
  return { key, crt };
}

module.exports = { findOpenssl, ensureCa, ensureDomainCert, CERTS_DIR };
