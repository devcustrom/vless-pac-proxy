import * as v from 'valibot'

const LABEL = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/
const TLD = /^[a-z]{2,24}$/

export function normalizeHostname(raw) {
  return String(raw || '').trim().replace(/\.$/, '').toLowerCase()
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
  v.toLowerCase(),
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

export const plainDomainSchema = v.object({ domain: domainSchema })

/** Оставляет только цифры (для input порта). */
export function digitsOnly(value) {
  return String(value || '').replace(/\D/g, '').slice(0, 5)
}
