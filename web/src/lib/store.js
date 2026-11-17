import { reactive } from 'vue'

export const store = reactive({
  status: null,
  offline: false,
  toast: null,
})

let toastTimer = null

export function notify(text, kind = 'ok') {
  store.toast = { text, kind }
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    store.toast = null
  }, 4500)
}

export async function api(path, body, method = 'POST') {
  const opts = { method, headers: {} }
  if (body !== undefined) {
    opts.headers['Content-Type'] = 'application/json'
    opts.headers['X-WPC'] = '1'
    opts.body = JSON.stringify(body)
  }
  const response = await fetch(path, opts)
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`)
  return data
}

export async function refresh() {
  try {
    store.status = await (await fetch('/api/status')).json()
    store.offline = false
  } catch {
    store.offline = true
  }
}

/** POST + обновление статуса; ошибки показываются тостом. */
export async function post(path, body) {
  try {
    const result = await api(path, body ?? {})
    await refresh()
    return result
  } catch (error) {
    notify(error.message, 'error')
    throw error
  }
}

export async function ensureEngines() {
  try {
    await api('/api/engines/ensure', {})
    notify('Загрузка движков запущена')
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  }
}

export async function installCerts() {
  try {
    await api('/api/trust-certs', {})
    notify('Запущен установщик сертификатов: подтвердите запрос системы')
  } catch (error) {
    notify(error.message, 'error')
  }
}

export function fmtTime(value) {
  return value ? new Date(value).toLocaleString('ru-RU') : '—'
}

export function startPolling() {
  refresh()
  setInterval(refresh, 5000)
}
