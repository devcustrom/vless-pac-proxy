import { mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test, expect } from '@playwright/test'

const PANEL = 'http://127.0.0.1:28180'
const LOCAL_HTTP = 28080
const SMART_PROXY = 28881

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
  await expect(page.getByRole('button', { name: 'Каналы' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Сайты' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Маршрутизация' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Локальные домены' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Telegram' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Обход DPI' })).toHaveCount(0)
})

test('смена основного канала обновляет кнопки без перезагрузки', async ({ page }) => {
  await page.getByText('Основной канал для остальных доменов').click()
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

test('автовыбор конфигов включает режим и запускает проверку', async ({ page }) => {
  let autoPayload = null
  let runPayload = null
  await page.route('**/api/auto', async (request) => {
    autoPayload = request.request().postDataJSON()
    await request.fulfill({ json: { ok: true } })
  })
  await page.route('**/api/configs/auto', async (request) => {
    runPayload = request.request().postDataJSON()
    await request.fulfill({ json: { ok: true } })
  })

  await page.getByRole('button', { name: 'Каналы' }).click()
  await expect(page.getByText('Автовыбор конфигураций')).toBeVisible()
  const toggle = page.getByTestId('auto-configs')
  await expect(toggle).toHaveAttribute('aria-checked', 'false')
  await toggle.click()
  await expect.poll(() => autoPayload).toEqual({ autoRotate: true })
  await page.getByRole('button', { name: 'Проверить и выбрать сейчас' }).click()
  await expect.poll(() => runPayload).toEqual({})
  await page.unrouteAll({ behavior: 'ignoreErrors' })
})

test('локальный домен отдаёт backend, ошибка не стирает форму', async ({ page }) => {
  await page.getByRole('button', { name: 'Локальные домены' }).click()
  await page.getByRole('button', { name: 'Добавить домен' }).click()
  const domain = page.getByPlaceholder(/app\.example\.com/)
  const port = page.getByPlaceholder('3000')

  await domain.fill(`http://loop.localhost:${LOCAL_HTTP}/some/path?q=1`)
  await page.getByRole('button', { name: 'Добавить', exact: true }).click()
  await expect(page.getByText('нельзя направить домен на сам локальный прокси')).toBeVisible()
  await expect(domain).toHaveValue('loop.localhost')
  await expect(port).toHaveValue(String(LOCAL_HTTP))

  const response = await page.request.get(`http://127.0.0.1:${LOCAL_HTTP}/`, {
    headers: { Host: `app.localhost:${LOCAL_HTTP}` },
  })
  expect(response.status()).toBe(200)
  expect(await response.text()).toContain('e2e local domain ok')
})

test('локальный домен работает через системный прокси', async ({ playwright }) => {
  const context = await playwright.request.newContext({
    proxy: { server: `http://127.0.0.1:${SMART_PROXY}` },
  })
  try {
    const response = await context.get('http://app.localhost/')
    expect(response.status()).toBe(200)
    expect(await response.text()).toContain('e2e local domain ok')
  } finally {
    await context.dispose()
  }
})

test('Telegram: невалидный порт блокирует сохранение', async ({ page }) => {
  await page.getByRole('button', { name: 'Telegram' }).click()
  await page.getByText('Дополнительно: сеть и sidecar', { exact: true }).click()
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
  await page.getByRole('combobox').click()
  await expect(page.getByRole('option', { name: 'В авто-списке' })).toBeVisible()
  await expect(page.getByRole('option', { name: 'Без проверки' })).toBeVisible()
  await page.keyboard.press('Escape')
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
  await page.getByRole('button', { name: 'Настройки' }).click()
  await page.getByRole('button', { name: 'Проверить обновления' }).click()
  await expect(page.getByRole('button', { name: 'Установить v9.9.9' })).toBeVisible()
})

test('в настройках показан HWID и кнопка копирования', async ({ page }) => {
  await page.getByRole('button', { name: 'Настройки' }).click()
  await page.getByText('HWID устройства').click()
  await expect(page.getByText('HWID устройства')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Копировать' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Сменить HWID' })).toBeVisible()
  await page.getByText('Проверка сайтов', { exact: true }).click()
  await expect(page.getByText('Автопроверка новых сайтов')).toBeVisible()
  await expect(page.getByText('Добавлять недоступные в авто-список')).toBeVisible()
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
  await expect(page.getByText('Zapret 200')).toBeVisible()
  await expect(page.getByText('Открыть в истории сайтов')).toBeVisible()
})

test('из проверки сайта можно добавить домен в авто-список', async ({ page }) => {
  await page.route('**/api/check', (route) =>
    route.fulfill({
      json: {
        domain: 'vpn-only.example',
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
  let payload = null
  await page.route('**/api/extra', async (route) => {
    payload = route.request().postDataJSON()
    await route.fulfill({ json: { ok: true } })
  })
  await page.getByPlaceholder('example.com').fill('vpn-only.example')
  await page.getByRole('button', { name: 'Проверить', exact: true }).click()
  await page.getByRole('button', { name: 'Добавить в авто' }).click()
  await expect.poll(() => payload).toEqual({ action: 'add', domain: 'vpn-only.example' })
})

test('Обход DPI: одна кнопка и список доменов', async ({ page }) => {
  test.skip(process.platform !== 'win32', 'Zapret доступен только на Windows')

  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: true } })
  await page.reload()
  await page.getByRole('button', { name: 'Обход DPI', exact: true }).click()

  await expect(page.getByRole('heading', { name: 'Обход DPI', exact: true })).toBeVisible()
  await expect(page.getByText('Домены для обхода')).toBeVisible()
  await expect(page.getByText('Берутся из общего списка доменов')).toBeVisible()
  await expect(page.getByRole('link', { name: 'Изменить в общем списке' })).toBeVisible()
  // Текст зависит от системной службы: у разработчика Zapret может быть
  // уже установлен и запущен, поэтому проверяем саму основную кнопку.
  await expect(page.getByTestId('zapret-primary')).toBeVisible()
  await expect(page.getByText('blockcheck2')).toHaveCount(0)

  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: false } })
})

test('Обход DPI предлагает пересобрать стратегию после смены списка', async ({ page }) => {
  test.skip(process.platform !== 'win32', 'Zapret доступен только на Windows')

  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, { headers, data: { zapret: true } })
  await page.route('**/api/status', async (route) => {
    const response = await route.fetch()
    const data = await response.json()
    data.zapret.strategyStale = true
    data.zapret.service = { ...data.zapret.service, installed: true, running: false }
    await route.fulfill({ response, json: data })
  })
  await page.reload()
  await page.getByRole('button', { name: 'Обход DPI', exact: true }).click()
  await expect(page.getByText('Список доменов изменился')).toBeVisible()

  let payload = null
  await page.route('**/api/zapret', async (route) => {
    payload = route.request().postDataJSON()
    await route.fulfill({ json: { ok: true } })
  })
  await page.getByRole('button', { name: 'Пересобрать стратегию' }).click()
  await expect.poll(() => payload?.action).toBe('auto-tune')
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
  expect(generated.scan).toBe('veryquick')

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

test('переключатель в недавних сайтах добавляет и убирает авто-маршрут', async ({ page }) => {
  const now = Math.floor(Date.now() / 1000)
  await page.route('**/api/status', async (request) => {
    const response = await request.fetch()
    const data = await response.json()
    data.visited = [
      {
        domain: 'example.com',
        count: 2,
        last: now,
        route: 'wireguard',
        routeSource: 'auto',
        autoRoute: 'wireguard',
        inAuto: true,
        checked: true,
      },
    ]
    data.checks = []
    await request.fulfill({ response, json: data })
  })

  let payload = null
  await page.route('**/api/extra', async (request) => {
    payload = request.request().postDataJSON()
    await request.fulfill({ json: { ok: true } })
  })

  await page.reload()
  await page.getByRole('button', { name: 'Сайты' }).click()
  const toggle = page.getByRole('switch', { name: 'Авто-маршрут' })
  await expect(toggle).toHaveAttribute('aria-checked', 'true')
  await expect(page.getByText('Авто · WireGuard')).toBeVisible()
  await toggle.click()
  await expect.poll(() => payload).not.toBeNull()
  expect(payload).toEqual({ action: 'remove', domain: 'example.com' })
  await expect(toggle).toHaveAttribute('aria-checked', 'false')

  await page.unrouteAll({ behavior: 'ignoreErrors' })
})

test('маршрутизация: один авто-список и маршруты по скорости', async ({ page }) => {
  const now = Math.floor(Date.now() / 1000)
  await page.route('**/api/status', async (request) => {
    const response = await request.fetch()
    const data = await response.json()
    data.extra = ['chatgpt.com', 'example.org', 'claude.ai']
    data.directDomains = ['lan.example']
    data.vpnOnly = ['claude.ai']
    data.autoRoutes = {
      'chatgpt.com': {
        route: 'wireguard',
        latencyMs: 120,
        viaZapret: false,
        checkedAt: now,
        direct: { code: '403', ms: 80 },
        wireguard: { code: '200', ms: 120 },
        vless: { code: '200', ms: 180 },
      },
      'example.org': {
        route: 'direct',
        latencyMs: 40,
        viaZapret: true,
        checkedAt: now,
        direct: { code: '200', ms: 40 },
      },
      'claude.ai': {
        route: 'direct',
        latencyMs: 30,
        viaZapret: false,
        checkedAt: now,
        direct: { code: '200', ms: 30 },
      },
    }
    await request.fulfill({ response, json: data })
  })
  let checked = null
  await page.route('**/api/routes/check', async (request) => {
    checked = request.request().postDataJSON()
    await request.fulfill({ json: { ok: true } })
  })
  let pinned = null
  await page.route('**/api/extra', async (request) => {
    pinned = request.request().postDataJSON()
    await request.fulfill({ json: { ok: true } })
  })

  await page.goto('/domains/routing')
  await expect(page.getByText('Домены с авто-маршрутом')).toBeVisible()
  await expect(page.getByText('Только через WireGuard')).toHaveCount(0)
  await expect(page.getByText('Только через VLESS')).toHaveCount(0)
  await expect(page.getByText(/WireGuard · 120 мс/)).toBeVisible()
  await expect(page.getByText(/Напрямую · Zapret · 40 мс/)).toBeVisible()
  await expect(page.getByText('Всегда VPN · WireGuard')).toBeVisible()
  await page.getByRole('button', { name: 'Всегда через VPN: claude.ai' }).click()
  await expect.poll(() => pinned).toEqual({
    action: 'vpn-only-remove',
    domain: 'claude.ai',
  })
  await page.getByRole('button', { name: 'Проверить все' }).click()
  await expect.poll(() => checked).toEqual({})
  await page.unrouteAll({ behavior: 'ignoreErrors' })
})

test('router: deep links, back/forward и выключенный плагин', async ({ page }) => {
  await page.goto('/channels')
  await expect(page).toHaveURL(/\/channels$/)
  await expect(page.getByRole('heading', { name: 'WireGuard — конфиги' })).toBeVisible()

  await page.getByRole('button', { name: 'Сайты' }).click()
  await expect(page.getByText('Недавние сайты')).toBeVisible()
  await page.goBack()
  await expect(page.getByRole('heading', { name: 'WireGuard — конфиги' })).toBeVisible()
  await page.goForward()
  await expect(page.getByText('Недавние сайты')).toBeVisible()

  await page.goto('/routing')
  await expect(page.getByRole('heading', { name: 'Маршрутизация доменов' })).toBeVisible()
  await page.goto('/domains/routing')
  await expect(page).toHaveURL(/\/routing$/)
  await expect(page.getByRole('heading', { name: 'Маршрутизация доменов' })).toBeVisible()

  // Старые hash-ссылки редиректятся на чистый путь.
  await page.goto('/#/channels')
  await expect(page).toHaveURL(/\/channels$/)

  await page.goto('/dpi')
  await expect(page.getByRole('heading', { name: 'Обход DPI', exact: true })).toBeVisible()
  if (process.platform === 'win32') {
    await expect(page.getByRole('button', { name: 'Включить в Настройках' })).toBeVisible()
    await page.getByRole('button', { name: 'Включить в Настройках' }).click()
    await expect(page.getByRole('heading', { name: 'Плагины' })).toBeVisible()
  } else {
    // На macOS Zapret недоступен, поэтому кнопки включения нет.
    await expect(
      page.getByText('Этот плагин доступен только на Windows.'),
    ).toBeVisible()
  }
})

test('Xray-конфиги добавляются, выбираются и удаляются', async ({ page }) => {
  const headers = { 'X-WPC': '1' }
  const cleanup = async () => {
    const response = await page.request.get(`${PANEL}/api/xray/configs`)
    const data = await response.json().catch(() => ({}))
    for (const item of data.configs || []) {
      await page.request.post(`${PANEL}/api/xray/configs`, {
        headers,
        data: { action: 'remove', slug: item.slug },
      })
    }
  }

  try {
    await page.goto('/channels')
    const card = page.getByTestId('xray-configs')
    await expect(card.getByRole('heading', { name: 'Xray: VLESS / Outline' })).toBeVisible()

    await card.getByRole('button', { name: 'Добавить конфиг' }).click()
    const form = page
      .locator('form')
      .filter({ has: page.getByPlaceholder('Например, основной VLESS') })
    await form.getByPlaceholder('Например, основной VLESS').fill('E2E Outline')
    await form
      .getByPlaceholder(/vless:/)
      .fill('ss://YWVzLTI1Ni1nY206cGFzcw==@1.2.3.4:8388#E2E')
    await form.getByRole('button', { name: 'Добавить конфиг' }).click()

    await expect(card.getByText('E2E Outline', { exact: true })).toBeVisible()
    await expect(card.getByText('Outline / SS', { exact: true }).first()).toBeVisible()
    await expect(card.getByText('активен', { exact: true })).toBeVisible()

    await page.route('**/api/xray/configs', async (route) => {
      const request = route.request()
      const body = request.method() === 'POST' ? request.postDataJSON() : null
      if (body?.action === 'test') {
        await route.fulfill({ json: { ok: true, ip: '203.0.113.9', ms: 42 } })
      } else {
        await route.continue()
      }
    })
    await card.getByRole('button', { name: 'Проверить все' }).click()
    await expect(card.getByText(/203\.0\.113\.9 · 42 мс/)).toBeVisible()
    await expect(card.getByText('Проверено 1 · рабочих 1')).toBeVisible()

    await page.request.post(`${PANEL}/api/xray/configs`, {
      headers,
      data: {
        action: 'add',
        name: 'E2E Second',
        link: 'ss://YWVzLTI1Ni1nY206cGFzcw==@5.6.7.8:8389#E2E2',
      },
    })
    await page.reload()
    const second = card.locator('li').filter({ hasText: 'E2E Second' })
    await expect(second).toBeVisible()
    await second.getByRole('button', { name: 'Использовать' }).click()
    await expect(second.getByText('активен', { exact: true })).toBeVisible()

    page.once('dialog', (dialog) => dialog.accept())
    await second.getByLabel('Действия с конфигом').click()
    await second.getByRole('button', { name: 'Удалить' }).click()
    await expect(second).toHaveCount(0)
    const first = card.locator('li').filter({ hasText: 'E2E Outline' })
    await expect(first.getByText('активен', { exact: true })).toBeVisible()

    page.once('dialog', (dialog) => dialog.accept())
    await first.getByLabel('Действия с конфигом').click()
    await first.getByRole('button', { name: 'Удалить' }).click()
    await expect(card.getByText('E2E Outline', { exact: true })).toHaveCount(0)
  } finally {
    await cleanup()
  }
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
  await page.goto('/dpi/catalog')
  await expect(page.getByRole('heading', { name: 'Каталог Zaprett' })).toBeVisible()
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

  await page.getByRole('button', { name: 'Скачать', exact: true }).first().click()
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

test('маршрутизация не зависит от плагина локальных доменов', async ({ page }) => {
  const headers = { 'X-WPC': '1' }
  await page.request.post(`${PANEL}/api/plugins`, {
    headers,
    data: { localDomains: false },
  })
  await page.reload()
  await expect(page.getByRole('button', { name: 'Маршрутизация' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Локальные домены' })).toHaveCount(0)
  await page.getByRole('button', { name: 'Маршрутизация' }).click()
  await expect(page.getByRole('heading', { name: 'Маршрутизация доменов' })).toBeVisible()
  await page.request.post(`${PANEL}/api/plugins`, {
    headers,
    data: { localDomains: true },
  })
})
