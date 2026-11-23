import * as v from 'valibot'

const LABEL = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/
const TLD = /^(?:[a-z]{2,24}|xn--[a-z0-9-]{2,59})$/

export function normalizeHostname(raw) {
  return String(raw || '').trim().replace(/\.$/, '').toLowerCase()
}

/**
 * Принимает домен или любую вставленную из браузера ссылку:
 * https://example.com/path?q=1 -> example.com
 * http://app.localhost:3000/ -> { domain: 'app.localhost', port: '3000' }
 */
export function parseDomainInput(raw) {
  let value = String(raw ?? '').trim()
  if (!value) return { domain: '', port: '' }
  value = value.replace(/^[\s<"'`]+/, '').replace(/[\s>"'`]+$/, '')
  const scheme = value.match(/^[a-z][a-z0-9+.-]*:\/\//i)
  if (scheme) {
    value = value.slice(scheme[0].length)
  } else if (value.startsWith('//')) {
    value = value.slice(2)
  }
  // userinfo@host
  value = value.replace(/^[^/?#@]*@/, '')
  // path, query и fragment не относятся к домену
  let authority = value.split(/[/?#]/, 1)[0]
  let port = ''
  if (!authority.startsWith('[')) {
    const match = authority.match(/^(.*):(\d{1,5})$/)
    if (match) {
      authority = match[1]
      port = match[2]
    }
  }
  let domain = authority.trim().replace(/\.$/, '').toLowerCase()
  if (domain && !/^[\x00-\x7f]*$/.test(domain)) {
    try {
      domain = new URL(`https://${domain}`).hostname
    } catch {
      /* оставляем как есть — isValidHostname отклонит */
    }
  }
  return { domain, port }
}

export function normalizeDomainInput(raw) {
  return parseDomainInput(raw).domain
}

/** Для поиска/фильтра: из вставленного URL оставляет только домен. */
export function normalizeDomainQuery(raw) {
  return normalizeDomainInput(raw)
}

export function isValidHostname(raw) {
  const value = normalizeHostname(raw)
  if (!value || value.length > 253) return false
  if (!/^[a-z0-9.-]+$/.test(value)) return false
  const labels = value.split('.')
  if (labels.length < 2 && value !== 'localhost') return false
  for (const label of labels) {
    if (!label || label.length > 63 || !LABEL.test(label)) return false
  }
  const tld = labels[labels.length - 1]
  return tld === 'localhost' || TLD.test(tld)
}

export const domainSchema = v.pipe(
  v.string(),
  v.trim(),
  v.transform(normalizeDomainInput),
  v.check(isValidHostname, 'Некорректный домен'),
)

export const portSchema = v.pipe(
  v.string(),
  v.trim(),
  v.regex(/^\d{1,5}$/, 'Только цифры'),
  v.transform(Number),
  v.minValue(1, 'Порт 1..65535'),
  v.maxValue(65535, 'Порт 1..65535'),
)

export const domainPortSchema = v.object({
  domain: domainSchema,
  port: portSchema,
})

export const wgConfigSchema = v.object({
  name: v.pipe(v.string(), v.trim(), v.minLength(1, 'Укажите имя')),
  content: v.pipe(
    v.string(),
    v.trim(),
    v.minLength(1, 'Вставьте wg.conf'),
    v.check((text) => text.includes('[Interface]') && text.includes('[Peer]'), 'Нужны секции [Interface] и [Peer]'),
  ),
})

export const xrayConfigSchema = v.object({
  name: v.pipe(v.string(), v.trim(), v.minLength(1, 'Укажите имя')),
  link: v.pipe(v.string(), v.trim(), v.minLength(1, 'Вставьте ссылку или JSON Outline')),
})

export const plainDomainSchema = v.object({ domain: domainSchema })

/** Оставляет только цифры (для input порта). */
export function digitsOnly(value) {
  return String(value || '').replace(/\D/g, '').slice(0, 5)
}
