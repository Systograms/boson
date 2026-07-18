import { describe, expect, it, vi } from 'vitest'
import { BosonApiError } from './errors.js'
import { createClient } from './client.js'

describe('BosonClient', () => {
  it('builds admin URLs and supplies the configured token', async () => {
    const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(
      new Response(JSON.stringify({ data: [] }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    )
    const client = createClient({
      baseUrl: 'https://api.example.test/',
      getToken: () => 'secret',
      fetch,
    })

    await client.admin.get<{ data: unknown[] }>('/users')

    expect(fetch).toHaveBeenCalledOnce()
    const [url, init] = fetch.mock.calls[0]!
    expect(url).toBe('https://api.example.test/admin/v1/users')
    expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer secret')
  })

  it('posts auth JSON and saves issued access tokens', async () => {
    const setToken = vi.fn()
    const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(
      Response.json({
        user: {},
        access_token: 'access',
        refresh_token: 'refresh',
        token_type: 'Bearer',
        expires_in: 900,
      }),
    )
    const client = createClient({ baseUrl: 'https://api.example.test', setToken, fetch })

    await client.auth.login({ email: 'me@example.test', password: 'long-enough' })

    expect(fetch.mock.calls[0]?.[0]).toBe('https://api.example.test/v1/auth/login')
    expect(setToken).toHaveBeenCalledWith('access')
  })

  it('parses structured API failures', async () => {
    const fetch = vi.fn<typeof globalThis.fetch>().mockResolvedValue(
      Response.json(
        { error: { code: 'identity.unauthorized', message: 'bad credentials' } },
        { status: 401 },
      ),
    )
    const client = createClient({ baseUrl: 'https://api.example.test', fetch })

    await expect(client.auth.me()).rejects.toEqual(
      expect.objectContaining<BosonApiError>({
        message: 'bad credentials',
        status: 401,
        code: 'identity.unauthorized',
      }),
    )
  })
})
