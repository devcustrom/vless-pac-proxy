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
    data: { directFirst: true },
  })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'wireproxy-control' })).toBeVisible()
})

test('вкладки показываются только для включённых плагинов', async ({ page }) => {
  await expect(page.getByRole('button', { name: 'Обзор' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Конфиги' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Домены' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Telegram' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Zapret' })).toHaveCount(0)
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
})
