import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  digitsOnly,
  domainSchema,
  isValidHostname,
  normalizeDomainInput,
  normalizeHostname,
  parseDomainInput,
  portSchema,
} from '../src/lib/validators.js'
import * as v from 'valibot'

test('digitsOnly оставляет только цифры и ограничивает 5 символами', () => {
  assert.equal(digitsOnly(' 18a0-80 '), '18080')
  assert.equal(digitsOnly('1234567'), '12345')
  assert.equal(digitsOnly(null), '')
})

test('hostname валидируется и нормализуется', () => {
  assert.equal(normalizeHostname(' APP.Example.COM. '), 'app.example.com')
  assert.equal(isValidHostname('app.example.com'), true)
  assert.equal(isValidHostname('panel.localhost'), true)
  assert.equal(isValidHostname('localhost'), true)
  assert.equal(isValidHostname('bad_domain.example.com'), false)
  assert.equal(isValidHostname('a..b'), false)
})

test('вставленная ссылка превращается в домен', () => {
  assert.equal(normalizeDomainInput('https://Example.COM/watch?v=1#x'), 'example.com')
  assert.equal(normalizeDomainInput('//sub.example.org/path'), 'sub.example.org')
  assert.equal(normalizeDomainInput('example.com:8443/path'), 'example.com')
  assert.equal(normalizeDomainInput('https://user:pass@example.com/'), 'example.com')
  assert.equal(normalizeDomainInput('https://пример.рф/путь'), 'xn--e1afmkfd.xn--p1ai')
  assert.deepEqual(parseDomainInput('http://app.localhost:3000/path'), {
    domain: 'app.localhost',
    port: '3000',
  })
  assert.equal(v.parse(domainSchema, 'https://www.youtube.com/watch?v=1'), 'www.youtube.com')
  assert.equal(v.parse(domainSchema, 'https://пример.рф/'), 'xn--e1afmkfd.xn--p1ai')
  assert.equal(isValidHostname('xn--e1afmkfd.xn--p1ai'), true)
})

test('portSchema принимает только 1..65535', () => {
  assert.equal(v.parse(portSchema, '8080'), 8080)
  assert.throws(() => v.parse(portSchema, '0'))
  assert.throws(() => v.parse(portSchema, '65536'))
  assert.throws(() => v.parse(portSchema, '80a'))
})
