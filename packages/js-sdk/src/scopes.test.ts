import { describe, expect, it } from 'vitest'
import { hasScope } from './scopes.js'

describe('hasScope', () => {
  it('matches bootstrap, wildcard, and exact scopes', () => {
    expect(hasScope({ bootstrap: true, scopes: [] }, 'admins:read')).toBe(true)
    expect(hasScope(['*'], 'admins:read')).toBe(true)
    expect(hasScope(['admins:read'], 'admins:read')).toBe(true)
  })

  it('rejects absent and partial scopes', () => {
    expect(hasScope(null, 'admins:read')).toBe(false)
    expect(hasScope(['admins'], 'admins:read')).toBe(false)
  })
})
