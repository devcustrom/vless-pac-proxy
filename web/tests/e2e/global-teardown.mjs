import { readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'

/** Playwright может убить webServer без SIGTERM — чистим профиль здесь. */
export default async function globalTeardown() {
  const dataDir = path.join(tmpdir(), 'wpc-e2e-data')
  await new Promise((resolve) => setTimeout(resolve, 500))
  try {
    for (const name of readdirSync(dataDir)) {
      rmSync(path.join(dataDir, name), { recursive: true, force: true, maxRetries: 10, retryDelay: 200 })
    }
  } catch {
    /* следующая E2E-сессия очистит профиль перед стартом */
  }
}
