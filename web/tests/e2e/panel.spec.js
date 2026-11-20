import { mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test, expect } from '@playwright/test'

const PANEL = 'http://127.0.0.1:28180'
const LOCAL_HTTP = 28080

test.beforeEach(async ({ page }) => {
  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/channels`, {
    headers,
    data: { channel: 'wireguard', enabled: false },
  })
  await page.request.post(`${PANEL}/api/channels`, {
    headers,
    data: { channel: 'xray', enabled: false },
  })
  await page.request.post(`${PANEL}/api/vless`, {
    headers,
    data: { action: 'set-default', value: 'wireguard' },
  })
  await page.request.post(`${PANEL}/api/auto`, {
    headers,
    data: { directFirst: true, autoCheckSites: true, autoAddUnavailable: true },
  })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'wireproxy-control' })).toBeVisible()
})

test('вкладки показываются только для включённых плагинов', async ({ page }) => {
  await expect(page.getByRole('button', { name: 'Обзор' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Конфиги' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Сайты' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Домены' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Telegram' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Обход DPI' })).toHaveCount(0)
})

test('polling обновляет переключатель без перезагрузки', async ({ page }) => {
  const direct = page.getByTestId('direct-first')
  await expect(direct).toHaveAttribute('aria-checked', 'true')
  const response = await page.request.post(`${PANEL}/api/auto`, {
    headers: { 'X-WPC': '1' },
    data: { directFirst: false },
  })
  expect(response.ok()).toBeTruthy()
  await expect(direct).toHaveAttribute('aria-checked', 'false', { timeout: 8_000 })
})

test('смена основного канала обновляет кнопки без перезагрузки', async ({ page }) => {
  const wireguard = page.getByRole('button', { name: 'WireGuard', exact: true })
  const vless = page.getByRole('button', { name: 'VLESS', exact: true })
  await wireguard.click()
  await expect(wireguard).toHaveClass(/bg-primary/)
  await vless.click()
  await expect(vless).toHaveClass(/bg-primary/)
})

test('переключатели каналов обновляются сразу', async ({ page }) => {
  const wg = page.getByTestId('wireguard-channel')
  const xray = page.getByTestId('xray-channel')
  await expect(wg).toHaveAttribute('aria-checked', 'false')
  await wg.click()
  await expect(wg).toHaveAttribute('aria-checked', 'true')
  await expect(xray).toHaveAttribute('aria-checked', 'false')
  await xray.click()
  await expect(xray).toHaveAttribute('aria-checked', 'true')
})

test('канал переключается оптимистично, не ожидая сервер', async ({ page }) => {
  await page.route('**/api/channels', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 1_500))
    await route.continue()
  })
  const wg = page.getByTestId('wireguard-channel')
  await expect(wg).toHaveAttribute('aria-checked', 'false')
  const response = page.waitForResponse('**/api/channels')
  await wg.click()
  await expect(wg).toHaveAttribute('aria-checked', 'true', { timeout: 500 })
  await response
})

test('локальный домен отдаёт backend, ошибка не стирает форму', async ({ page }) => {
  await page.getByRole('button', { name: 'Домены' }).click()
  const domain = page.getByPlaceholder('app.example.com')
  const port = page.getByPlaceholder('3000')

  await domain.fill('loop.localhost')
  await port.fill(String(LOCAL_HTTP))
  await page.getByRole('button', { name: 'Добавить', exact: true }).click()
  await expect(page.getByText('нельзя направить домен на сам локальный прокси')).toBeVisible()
  await expect(domain).toHaveValue('loop.localhost')

  const response = await page.request.get(`http://127.0.0.1:${LOCAL_HTTP}/`, {
    headers: { Host: `app.localhost:${LOCAL_HTTP}` },
  })
  expect(response.status()).toBe(200)
  expect(await response.text()).toContain('e2e local domain ok')
})

test('Telegram: невалидный порт блокирует сохранение', async ({ page }) => {
  await page.getByRole('button', { name: 'Telegram' }).click()
  const port = page.getByTestId('telegram-port')
  await expect(port).toBeVisible()
  await port.fill('')
  await expect(page.getByRole('button', { name: 'Применить', exact: true })).toBeDisabled()
  await port.fill('1443')
  await expect(page.getByRole('button', { name: 'Применить', exact: true })).toBeEnabled()
})

test('документация открывается и переключает вкладки', async ({ page }) => {
  await page.getByRole('button', { name: 'Документация' }).click()
  await expect(page.getByRole('heading', { name: 'Документация' })).toBeVisible()
  await page.getByRole('button', { name: 'Плагины и протоколы' }).click()
  await expect(page.locator('.markdown')).toContainText('плагин')
  await page.getByRole('button', { name: 'Закрыть' }).click()
  await expect(page.getByRole('heading', { name: 'Документация' })).toHaveCount(0)
})

test('вкладка сайтов показывает фильтры и проверку списка', async ({ page }) => {
  await page.getByRole('button', { name: 'Сайты' }).click()
  await expect(page.getByText('Недавние сайты')).toBeVisible()
  await expect(page.getByText('Через VPN')).toBeVisible()
  await expect(page.getByText('Без проверки')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Проверить все' })).toBeVisible()
})

test('доступное обновление показывает кнопку установки', async ({ page }) => {
  await page.route('**/api/update', (route) =>
    route.fulfill({
      json: {
        current: '0.5.4',
        latest: '9.9.9',
        available: true,
        asset: 'wireproxy-control-windows-x64.exe',
        url: 'https://example.com/app.exe',
        page: 'https://example.com',
      },
    }),
  )
  await page.getByRole('button', { name: 'Проверить обновления' }).click()
  await expect(page.getByRole('button', { name: 'Установить v9.9.9' })).toBeVisible()
})

test('в настройках показан HWID и кнопка копирования', async ({ page }) => {
  await page.getByRole('button', { name: 'Настройки' }).click()
  await expect(page.getByText('HWID устройства')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Копировать' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Сменить HWID' })).toBeVisible()
  await expect(page.getByText('Автопроверка новых сайтов')).toBeVisible()
  await expect(page.getByText('Добавлять недоступные в VPN')).toBeVisible()
  await expect(page.getByText('Резервная копия')).toBeVisible()
  await expect(page.getByText('Скачать бэкап')).toBeVisible()
})

test('проверка сайта показывает результат Zapret', async ({ page }) => {
  await page.route('**/api/check', (route) =>
    route.fulfill({
      json: {
        domain: 'example.com',
        direct: '000',
        vpn: '200',
        zapret: '200',
        zapretRunning: true,
        zapretSupported: true,
        verdict: 'vpn-helps',
        at: new Date().toISOString(),
      },
    }),
  )
  await page.getByPlaceholder('example.com').fill('example.com')
  await page.getByRole('button', { name: 'Проверить', exact: true }).click()
  await expect(page.getByText('Через Zapret:')).toBeVisible()
  await expect(page.getByText('Zapret работает прозрачно')).toBeVisible()
})

test('несколько стратегий Zapret выбираются и уходят одним набором', async ({ page }) => {
  test.skip(process.platform !== 'win32', 'Zapret доступен только на Windows')

  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: true } })
  await page.reload()
  await page.getByRole('button', { name: 'Обход DPI', exact: true }).click()
  await expect(page.getByText('Стратегии Zapret')).toBeVisible()

  const first = page.getByRole('checkbox', { name: 'general', exact: true })
  const second = page.getByRole('checkbox', { name: 'general-alt', exact: true })
  await expect(first).toBeChecked()
  await expect(second).not.toBeChecked()
  await second.check()

  let payload = null
  await page.route('**/api/zapret', async (route) => {
    if (route.request().method() === 'POST') {
      payload = route.request().postDataJSON()
      await route.fulfill({ json: { ok: true } })
      return
    }
    await route.continue()
  })
  await page.getByRole('button', { name: /Установить службу|Применить пресет/ }).click()
  await expect.poll(() => payload).not.toBeNull()
  expect(payload.action).toBe('install')
  expect(payload.presets).toEqual(['general.bat', 'general-alt.bat'])
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: false } })
})

test('из проверки сайта можно сгенерировать и сохранить стратегии', async ({ page }) => {
  test.skip(process.platform !== 'win32', 'Zapret доступен только на Windows')

  const zapret = path.join(tmpdir(), 'wpc-e2e-data', 'zapret')
  const engine = path.join(zapret, 'zapret-winws')
  mkdirSync(engine, { recursive: true })
  for (const name of ['winws2.exe', 'WinDivert.dll', 'WinDivert64.sys']) {
    writeFileSync(path.join(engine, name), '')
  }
  mkdirSync(path.join(zapret, 'cygwin', 'bin'), { recursive: true })
  writeFileSync(path.join(zapret, 'cygwin', 'bin', 'bash.exe'), '')
  writeFileSync(path.join(zapret, 'cygwin', 'bin', 'cygpath.exe'), '')
  mkdirSync(path.join(zapret, 'blockcheck', 'zapret2'), { recursive: true })
  writeFileSync(path.join(zapret, 'blockcheck', 'zapret2', 'blockcheck2.sh'), '')

  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: true } })

  let autoState = null
  await page.route('**/api/status', async (route) => {
    const response = await route.fetch()
    const data = await response.json()
    if (autoState) data.zapret.autoTune = autoState
    await route.fulfill({ response, json: data })
  })
  await page.reload()

  await page.route('**/api/check', (route) =>
    route.fulfill({
      json: {
        domain: 'example.com',
        direct: '000',
        vpn: '200',
        zapret: 'n/a',
        zapretRunning: false,
        zapretSupported: true,
        verdict: 'vpn-helps',
        at: new Date().toISOString(),
      },
    }),
  )

  let generated = null
  let saved = null
  let applied = null
  await page.route('**/api/zapret', async (route) => {
    if (route.request().method() !== 'POST') {
      await route.continue()
      return
    }
    const body = route.request().postDataJSON()
    if (body.action === 'generate-strategies') {
      generated = body
      await route.fulfill({ json: { ok: true } })
      return
    }
    if (body.action === 'save-strategy') {
      saved = body
      await route.fulfill({ json: { ok: true, preset: 'preset2_gen_example.com.cmd' } })
      return
    }
    if (body.action === 'apply-strategy') {
      applied = body
      await route.fulfill({ json: { ok: true } })
      return
    }
    await route.continue()
  })

  await page.getByPlaceholder('example.com').fill('example.com')
  await page.getByRole('button', { name: 'Проверить', exact: true }).click()
  await page.getByRole('button', { name: 'Подобрать стратегии Zapret' }).click()

  await expect(page.getByRole('heading', { name: 'Стратегии для example.com' })).toBeVisible()
  await expect.poll(() => generated).not.toBeNull()
  expect(generated.domain).toBe('example.com')
  expect(generated.scan).toBe('quick')

  autoState = {
    running: false,
    phase: 'done',
    log: 'blockcheck summary',
    error: null,
    strategy: null,
    domain: 'example.com',
    candidates: [
      {
        index: 0,
        label: 'curl_test_https_tls13 ipv4 example.com',
        strategy: '--wf-tcp-out=443 --filter-tcp=443 --lua-desync=fake',
      },
    ],
    preset: null,
  }
  await expect(page.getByText('Найдено стратегий: 1')).toBeVisible({ timeout: 12_000 })

  await page.getByRole('button', { name: 'В пресеты' }).click()
  await expect.poll(() => saved).not.toBeNull()
  expect(saved.index).toBe(0)

  await page.getByRole('button', { name: 'Применить', exact: true }).click()
  await expect.poll(() => applied).not.toBeNull()
  expect(applied.index).toBe(0)

  await page.unrouteAll({ behavior: 'ignoreErrors' })
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: false } })
})

test('каталог Zaprett показывает источник и умеет скачать/применить', async ({ page }) => {
  test.skip(process.platform !== 'win32', 'Zapret доступен только на Windows')

  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: true } })

  const source = 'https://github.com/CherretGit/zaprett-repo'
  const catalog = {
    cached: true,
    source: `${source.replace('github.com', 'raw.githubusercontent.com')}/refs/heads/main/index.json`,
    sourcePage: source,
    updatedAt: new Date().toISOString(),
    engineV1: true,
    engineV2: true,
    selectedLists: [],
    selectedIpsets: [],
    active: '',
    licenseNote: 'Данные загружаются из внешнего репозитория.',
    strategies: [
      {
        id: 'strategy-default',
        kind: 'nfqws',
        name: 'strategy-default',
        description: 'Стандартная стратегия',
        author: 'zaprett-devs',
        version: '1.0.0',
        source: `${source}/blob/main/manifests/strategies/nfqws/strategy-default.json`,
        artifact: `${source}/blob/main/files/strategies/nfqws/strategy-default.txt`,
        downloaded: false,
        ready: false,
        active: false,
        supported: true,
        dependencies: 2,
      },
    ],
    lists: [
      {
        id: 'list-youtube',
        kind: 'list',
        name: 'list-youtube',
        description: 'Домены YouTube',
        author: 'zaprett-devs',
        version: '1.0.0',
        source: `${source}/blob/main/manifests/lists/include/list-youtube.json`,
        downloaded: true,
        ready: true,
        active: false,
        supported: false,
        dependencies: 0,
      },
    ],
    ipsets: [],
    bins: [],
    test: {
      running: false,
      startedAt: new Date().toISOString(),
      finishedAt: new Date().toISOString(),
      done: 1,
      total: 1,
      current: '',
      error: null,
      results: [
        {
          id: 'strategy-default',
          name: 'strategy-default',
          kind: 'nfqws',
          author: 'zaprett-devs',
          reachable: ['example.com'],
          codes: [{ domain: 'example.com', code: '200', ms: 120 }],
          error: null,
        },
      ],
    },
  }
  await page.route('**/api/status', async (route) => {
    const response = await route.fetch()
    const data = await response.json()
    data.zapret.catalog = catalog
    await route.fulfill({ response, json: data })
  })
  await page.reload()
  await page.getByRole('button', { name: 'Обход DPI', exact: true }).click()
  await expect(page.getByText('Каталог Zaprett')).toBeVisible()
  await expect(page.getByRole('link', { name: source })).toBeVisible()
  await expect(page.getByText('Стандартная стратегия')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Проверить скачанные' })).toBeVisible()
  await expect(page.getByText('1 / 1')).toBeVisible()

  let downloaded = null
  let applied = null
  let selected = null
  await page.route('**/api/zapret', async (route) => {
    if (route.request().method() !== 'POST') {
      await route.continue()
      return
    }
    const body = route.request().postDataJSON()
    if (body.action === 'catalog-download') {
      downloaded = body
      catalog.strategies[0].downloaded = true
      catalog.strategies[0].ready = true
      await route.fulfill({ json: { ok: true, downloaded: 3 } })
      return
    }
    if (body.action === 'catalog-select') {
      selected = body
      catalog.selectedLists = body.lists
      await route.fulfill({ json: { ok: true } })
      return
    }
    if (body.action === 'catalog-apply') {
      applied = body
      catalog.active = body.id
      await route.fulfill({ json: { ok: true } })
      return
    }
    await route.continue()
  })

  await page.getByRole('button', { name: 'Скачать' }).first().click()
  await expect.poll(() => downloaded).not.toBeNull()
  expect(downloaded.id).toBe('strategy-default')

  await page.getByRole('button', { name: /^Списки/ }).click()
  await page.getByRole('checkbox', { name: /использовать в hostlists/ }).check()
  await page.getByRole('button', { name: 'Сохранить выбор' }).click()
  await expect.poll(() => selected).not.toBeNull()
  expect(selected.lists).toEqual(['list-youtube'])

  await page.getByRole('button', { name: /^Стратегии/ }).click()
  page.once('dialog', (dialog) => dialog.accept())
  await page.getByRole('button', { name: 'Применить', exact: true }).last().click()
  await expect.poll(() => applied).not.toBeNull()
  expect(applied.id).toBe('strategy-default')

  await page.unrouteAll({ behavior: 'ignoreErrors' })
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: false } })
})
