import { test, expect } from '@playwright/test'

const PANEL = 'http://127.0.0.1:28180'
const LOCAL_HTTP = 28080

test.beforeEach(async ({ page }) => {
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
