import { spawn } from 'node:child_process'
import { createServer } from 'node:http'
import { mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const webRoot = path.resolve(here, '../..')
const repoRoot = path.resolve(webRoot, '..')
const binary = process.env.WPC_E2E_BIN
  || path.join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'wireproxy-control.exe' : 'wireproxy-control')

const PANEL_PORT = 28180
const LOCAL_HTTP = 28080
const LOCAL_HTTPS = 28443
const BACKEND_PORT = 28081
// Фиксированный путь: если прошлый запуск убили без cleanup, следующий
// запуск удалит профиль и начнёт с чистого листа.
const dataDir = path.join(tmpdir(), 'wpc-e2e-data')
mkdirSync(dataDir, { recursive: true })
for (const name of readdirSync(dataDir)) {
  rmSync(path.join(dataDir, name), { recursive: true, force: true, maxRetries: 5, retryDelay: 200 })
}
const settings = {
  vpnDefault: 'wireguard',
  autoRotate: false,
  autoPoolRefreshHours: 0,
  localProxyPort: LOCAL_HTTP,
  localHttpsPort: LOCAL_HTTPS,
  localDomainsEnabled: true,
  wireguardEnabled: false,
  xrayEnabled: false,
  onboardingDone: true,
  telegramEnabled: true,
  telegramHost: '127.0.0.1',
  telegramPort: 15443,
  telegramSecret: '0123456789abcdef0123456789abcdef',
  telegramTryDirect: true,
  telegramOrder: 'auto',
  telegramAutoUpdate: false,
  zapretEnabled: false,
  directFirst: true,
}
writeFileSync(path.join(dataDir, 'settings.json'), JSON.stringify(settings, null, 2))
writeFileSync(path.join(dataDir, 'local-domains.txt'), `app.localhost = ${BACKEND_PORT}\n`)

// Фиктивный бандл Zapret для проверки выбора нескольких стратегий в UI.
// Служба не устанавливается, а запрос на установку тест перехватывает.
const zapretDir = path.join(dataDir, 'zapret')
mkdirSync(path.join(zapretDir, 'bin'), { recursive: true })
for (const name of ['bin/winws.exe', 'bin/WinDivert.dll', 'bin/WinDivert64.sys']) {
  writeFileSync(path.join(zapretDir, name), '')
}
writeFileSync(
  path.join(zapretDir, 'general.bat'),
  '@echo off\r\nstart "" "%~dp0bin\\winws.exe" --wf-tcp=80,443 --filter-tcp=443 --dpi-desync=fake --new\r\n',
)
writeFileSync(
  path.join(zapretDir, 'general-alt.bat'),
  '@echo off\r\nstart "" "%~dp0bin\\winws.exe" --wf-udp=443 --filter-udp=443 --dpi-desync=fake --new\r\n',
)

const backend = createServer((_req, res) => {
  res.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' })
  res.end('e2e local domain ok')
})
await new Promise((resolve, reject) => {
  backend.once('error', reject)
  backend.listen(BACKEND_PORT, '127.0.0.1', resolve)
})

const app = spawn(binary, [
  '--data-dir', dataDir,
  '--panel-port', String(PANEL_PORT),
  '--no-tray',
], {
  cwd: dataDir,
  env: { ...process.env, WPC_TEST_MODE: '1' },
  stdio: ['ignore', 'pipe', 'pipe'],
})

for (const stream of [app.stdout, app.stderr]) {
  stream.setEncoding('utf8')
  stream.on('data', (chunk) => process.stdout.write(`[app] ${chunk}`))
}

let stopping = false
async function stop(code) {
  if (stopping) return
  stopping = true
  const exited = new Promise((resolve) => app.once('exit', resolve))
  try { app.kill() } catch {}
  await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 2_000))])
  try { backend.close() } catch {}
  try {
    for (const name of readdirSync(dataDir)) {
      rmSync(path.join(dataDir, name), { recursive: true, force: true, maxRetries: 5, retryDelay: 200 })
    }
  } catch {}
  process.exit(code)
}

app.on('exit', (code) => {
  if (!stopping) {
    console.error(`e2e: manager exited unexpectedly (code ${code})`)
    stop(1)
  }
})
process.on('SIGTERM', () => stop(0))
process.on('SIGINT', () => stop(0))

const statusUrl = `http://127.0.0.1:${PANEL_PORT}/api/status`
let ready = false
for (let i = 0; i < 240; i += 1) {
  try {
    const response = await fetch(statusUrl)
    if (response.ok) {
      ready = true
      break
    }
  } catch {
    /* менеджер ещё стартует */
  }
  await new Promise((resolve) => setTimeout(resolve, 250))
}
if (!ready) {
  console.error(`e2e: panel did not start: ${statusUrl}`)
  await stop(1)
}
console.log(`e2e: panel ${statusUrl}, local http ${LOCAL_HTTP}, backend ${BACKEND_PORT}`)
setInterval(() => {}, 2 ** 30)
