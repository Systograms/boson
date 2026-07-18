import { createClient } from '@boson/sdk'

export * from '@boson/sdk'
export { BosonApiError as AdminApiError } from '@boson/sdk'

declare global {
  interface Window {
    __BOSON_CONFIG__?: {
      apiUrl?: string
    }
  }
}

function resolveServerUrl(): string {
  const configured = window.__BOSON_CONFIG__?.apiUrl
  if (configured !== undefined && configured !== '') {
    return configured
  }
  if (import.meta.env.VITE_BOSON_URL) {
    return import.meta.env.VITE_BOSON_URL
  }
  if (typeof window !== 'undefined' && window.location?.origin) {
    const port = window.location.port
    // Vite / standalone dashboard local ports still talk to the Server.
    if (port === '5173' || port === '3000') {
      return 'http://localhost:8080'
    }
    return window.location.origin
  }
  return 'http://localhost:8080'
}

export const SERVER_URL: string = resolveServerUrl()

const TOKEN_KEY = 'boson.admin_token'

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? ''
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token)
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY)
}

const boson = createClient({
  baseUrl: SERVER_URL,
  getToken,
  setToken,
})

export function adminGet<T>(endpoint: string): Promise<T> {
  return boson.admin.get<T>(endpoint)
}

export function adminPost<T>(endpoint: string, body: unknown): Promise<T> {
  return boson.admin.post<T>(endpoint, body)
}
