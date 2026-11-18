import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  digitsOnly,
  isValidHostname,
  normalizeHostname,
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

test('portSchema принимает только 1..65535', () => {
  assert.equal(v.parse(portSchema, '8080'), 8080)
  assert.throws(() => v.parse(portSchema, '0'))
  assert.throws(() => v.parse(portSchema, '65536'))
  assert.throws(() => v.parse(portSchema, '80a'))
})
