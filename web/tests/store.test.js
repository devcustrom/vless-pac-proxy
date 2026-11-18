import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

// store.js использует window только для события обновления статуса.
globalThis.window = new EventTarget()

const { store, refresh, post } = await import('../src/lib/store.js')

let refreshEvents = 0
window.addEventListener('wpc:status-refresh', () => {
  refreshEvents += 1
})

beforeEach(() => {
  refreshEvents = 0
  store.offline = false
})

test('refresh обновляет store и просит App перезапросить статус', async () => {
  globalThis.fetch = async (url) => {
    assert.equal(url, '/api/status')
    return { ok: true, json: async () => ({ version: 'test', wgUp: true }) }
  }
  await refresh()
  assert.equal(store.status.version, 'test')
  assert.equal(store.offline, false)
  assert.equal(refreshEvents, 1)
})

test('refresh при ошибке включает offline и не рассылает событие', async () => {
  globalThis.fetch = async () => {
    throw new Error('down')
  }
  await refresh()
  assert.equal(store.offline, true)
  assert.equal(refreshEvents, 0)
})

test('post после успешного POST обновляет статус без перезагрузки', async () => {
  const calls = []
  globalThis.fetch = async (url, options) => {
    calls.push({ url, options })
    if (url === '/api/status') {
      return { ok: true, json: async () => ({ version: 'test' }) }
    }
    return { ok: true, json: async () => ({ ok: true }) }
  }
  const result = await post('/api/auto', { directFirst: false })
  assert.deepEqual(result, { ok: true })
  assert.equal(calls.length, 2)
  assert.equal(calls[0].options.headers['X-WPC'], '1')
  assert.equal(refreshEvents, 1)
})
