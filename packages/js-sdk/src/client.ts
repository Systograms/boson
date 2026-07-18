import { AdminClient } from './admin.js'
import { AuthClient } from './auth.js'
import { BosonApiError, parseErrorBody } from './errors.js'

export type TokenGetter = () => string | null | undefined | Promise<string | null | undefined>
export type TokenSetter = (token: string) => void | Promise<void>

export type BosonClientConfig = {
  baseUrl: string
  getToken?: TokenGetter
  setToken?: TokenSetter
  fetch?: typeof globalThis.fetch
}

export class BosonClient {
  readonly auth: AuthClient
  readonly admin: AdminClient
  readonly baseUrl: string
  private readonly getToken?: TokenGetter
  private readonly setToken?: TokenSetter
  private readonly fetcher: typeof globalThis.fetch

  constructor(config: BosonClientConfig) {
    this.baseUrl = config.baseUrl.replace(/\/+$/, '')
    this.getToken = config.getToken
    this.setToken = config.setToken
    this.fetcher = config.fetch ?? globalThis.fetch
    if (!this.fetcher) throw new Error('A Fetch API implementation is required')
    this.auth = new AuthClient(this)
    this.admin = new AdminClient(this)
  }

  url(path: string): string {
    return `${this.baseUrl}/${path.replace(/^\/+/, '')}`
  }

  async saveToken(token: string): Promise<void> {
    await this.setToken?.(token)
  }

  async request<T>(
    path: string,
    init: RequestInit = {},
    options: { authenticated?: boolean } = {},
  ): Promise<T> {
    const headers = new Headers(init.headers)
    const token = options.authenticated === false ? null : await this.getToken?.()
    if (token && !headers.has('Authorization')) headers.set('Authorization', `Bearer ${token}`)
    if (init.body != null && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }

    let response: Response
    try {
      response = await this.fetcher(this.url(path), { ...init, headers })
    } catch (cause) {
      throw new BosonApiError('Unable to reach the Boson server', { cause })
    }

    if (!response.ok) {
      const fallback = `Boson API returned ${response.status}`
      let parsed: { code?: string; message?: string } = {}
      try {
        parsed = parseErrorBody(await response.json())
      } catch {
        // Preserve the status-based fallback for empty or non-JSON responses.
      }
      throw new BosonApiError(parsed.message ?? fallback, {
        status: response.status,
        code: parsed.code,
      })
    }

    const body = await response.text()
    if (!body) return undefined as T
    return JSON.parse(body) as T
  }
}

export function createClient(config: BosonClientConfig): BosonClient {
  return new BosonClient(config)
}
