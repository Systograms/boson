import { describe, expect, it } from 'vitest'
import { BosonApiError, parseErrorBody } from './errors.js'

describe('parseErrorBody', () => {
  it('extracts structured Boson errors', () => {
    expect(
      parseErrorBody({ error: { code: 'identity.invalid', message: 'invalid request' } }),
    ).toEqual({ code: 'identity.invalid', message: 'invalid request' })
  })

  it('accepts legacy string errors and ignores malformed bodies', () => {
    expect(parseErrorBody({ error: 'failed' })).toEqual({ message: 'failed' })
    expect(parseErrorBody({ nope: true })).toEqual({})
  })
})

describe('BosonApiError', () => {
  it('retains status and code', () => {
    const error = new BosonApiError('forbidden', { status: 403, code: 'admin.forbidden' })
    expect(error.status).toBe(403)
    expect(error.code).toBe('admin.forbidden')
  })
})
