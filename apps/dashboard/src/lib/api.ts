export const SERVER_URL: string =
  import.meta.env.VITE_BOSON_URL ?? 'http://localhost:8080'

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

export class AdminApiError extends Error {
  readonly status?: number

  constructor(message: string, status?: number) {
    super(message)
    this.status = status
  }
}

export async function adminGet<T>(endpoint: string): Promise<T> {
  let response: Response
  try {
    response = await fetch(`${SERVER_URL}/admin/v1/${endpoint}`, {
      headers: { Authorization: `Bearer ${getToken()}` },
    })
  } catch {
    throw new AdminApiError('Unable to reach the Boson server')
  }
  if (!response.ok) {
    throw new AdminApiError(`Admin API returned ${response.status}`, response.status)
  }
  return response.json() as Promise<T>
}

export type HealthCheck = {
  name: string
  status: string
  message: string | null
}

export type Health = {
  status: string
  version: string
  checks: HealthCheck[]
}

export type Overview = {
  metrics: {
    total_requests: number
    total_errors: number
    error_rate: number
    retained_traces: number
  }
  workers: Array<{ name: string; last_heartbeat: string }>
}

export type RequestTrace = {
  request_id: string
  started_at: string
  method: string
  path: string
  status_code: number
  duration_ms: number
}

export type AdminConfig = {
  snapshot_id: string
  effective: Record<string, unknown>
  read_only: boolean
}
