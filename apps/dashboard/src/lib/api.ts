declare global {
  interface Window {
    __BOSON_CONFIG__?: {
      apiUrl?: string
    }
  }
}

export const SERVER_URL: string =
  window.__BOSON_CONFIG__?.apiUrl ??
  import.meta.env.VITE_BOSON_URL ??
  'http://localhost:8080'

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

async function adminRequest<T>(
  endpoint: string,
  init?: RequestInit,
): Promise<T> {
  let response: Response
  try {
    response = await fetch(`${SERVER_URL}/admin/v1/${endpoint}`, {
      ...init,
      headers: {
        Authorization: `Bearer ${getToken()}`,
        ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
        ...init?.headers,
      },
    })
  } catch {
    throw new AdminApiError('Unable to reach the Boson server')
  }
  if (!response.ok) {
    let message = `Admin API returned ${response.status}`
    try {
      const body = (await response.json()) as {
        error?: { message?: string } | string
      }
      if (typeof body.error === 'string') message = body.error
      else if (body.error?.message) message = body.error.message
    } catch {
      // Keep the status-based fallback for empty or non-JSON responses.
    }
    throw new AdminApiError(message, response.status)
  }
  return response.json() as Promise<T>
}

export function adminGet<T>(endpoint: string): Promise<T> {
  return adminRequest<T>(endpoint)
}

export function adminPost<T>(
  endpoint: string,
  body: unknown,
): Promise<T> {
  return adminRequest<T>(endpoint, {
    method: 'POST',
    body: JSON.stringify(body),
  })
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

export type CorrelatedEvent = {
  id: string
  topic: string
  status: string
  attempts: number
  occurred_at: string
  dispatched_at: string | null
  last_error: string | null
}

export type CorrelatedJob = {
  id: string
  topic: string
  status: string
  attempts: number
  created_at: string
  updated_at: string
  last_error: string | null
}

export type RequestDetail = {
  request: RequestTrace
  events: CorrelatedEvent[]
  jobs: CorrelatedJob[]
}

export type EndUser = {
  id: string
  email: string
  display_name: string
  email_verified_at: string | null
  created_at: string
}

export type EndUserSession = {
  id: string
  user_id: string
  expires_at: string
  revoked_at: string | null
  last_used_at: string | null
  created_at: string
}

export type AdminFile = {
  id: string
  owner_id: string
  filename: string
  content_type: string
  size_bytes: number
  status: string
  created_at: string
  updated_at: string
  deleted_at: string | null
}

export type AuditEntry = {
  event_id: string
  topic: string
  payload: unknown
  correlation_id: string | null
  occurred_at: string
  recorded_at: string
}

export type NotificationDelivery = {
  event_id: string
  kind: string
  recipient: string
  subject: string
  status: 'pending' | 'sent' | 'failed'
  attempts: number
  last_error: string | null
  created_at: string
  last_attempted_at: string | null
  sent_at: string | null
}

export type AdminConfig = {
  snapshot_id: string
  effective: Record<string, unknown>
  read_only: boolean
}

export type AdminUser = {
  id: string
  email: string
  display_name: string
  disabled_at: string | null
  created_at: string
}

export type AdminApiKey = {
  id: string
  admin_id: string
  name: string
  token_prefix: string
  scopes: string[]
  last_used_at: string | null
  expires_at: string | null
  revoked_at: string | null
  created_at: string
}

export type AdminPrincipal = {
  admin_id: string | null
  email: string | null
  scopes: string[]
  bootstrap: boolean
}

export type IssuedCredential = {
  admin: AdminUser
  key: AdminApiKey
  token: string
}

export type IssuedApiKey = Pick<
  AdminApiKey,
  'id' | 'admin_id' | 'name' | 'token_prefix' | 'scopes'
> & {
  token: string
}

export type Organization = {
  id: string
  name: string
  slug: string
  created_by: string
  member_count: number
  created_at: string
  updated_at: string
}

export type OrganizationMembership = {
  organization_id: string
  user_id: string
  role: 'owner' | 'admin' | 'member'
  joined_at: string
  updated_at: string
}

export type OrganizationInvitation = {
  id: string
  organization_id: string
  email: string
  role: 'owner' | 'admin' | 'member'
  expires_at: string
  accepted_at: string | null
  revoked_at: string | null
  invited_by: string
  created_at: string
}

export type JobRecord = {
  envelope: {
    id: string
    topic: string
    payload: unknown
    attempts: number
    max_attempts: number
    correlation_id: string | null
  }
  status: 'pending' | 'running' | 'completed' | 'failed' | 'dead'
  run_at: string
  locked_at: string | null
  locked_by: string | null
  last_error: string | null
  created_at: string
  updated_at: string
}

export type EventRecord = {
  id: string
  topic: string
  payload: unknown
  correlation_id: string | null
  occurred_at: string
  status: 'pending' | 'processing' | 'dispatched'
  attempts: number
  run_at: string
  locked_at: string | null
  locked_by: string | null
  last_error: string | null
  dispatched_at: string | null
  created_at: string
}

export type EventDelivery = {
  consumer: string
  status: 'pending' | 'succeeded' | 'failed'
  attempts: number
  last_error: string | null
  first_attempted_at: string | null
  last_attempted_at: string | null
  delivered_at: string | null
}

export type EventDetail = {
  event: EventRecord
  deliveries: EventDelivery[]
}

export type DatabaseTableRef = {
  namespace: string
  name: string
}

export type DatabaseRowCount = {
  value: number
  exact: boolean
}

export type DatabaseTableSummary = {
  table: DatabaseTableRef
  primary_key: string[]
  row_count: DatabaseRowCount | null
}

export type DatabaseColumn = {
  name: string
  data_type: string
  nullable: boolean
  primary_key: boolean
  redacted: boolean
  default: string | null
}

export type DatabaseForeignKey = {
  name: string
  columns: string[]
  referenced_table: DatabaseTableRef
  referenced_columns: string[]
}

export type DatabaseTableSchema = {
  table: DatabaseTableRef
  columns: DatabaseColumn[]
  primary_key: string[]
  foreign_keys: DatabaseForeignKey[]
  row_count: DatabaseRowCount | null
}

export type DatabaseCell = {
  kind:
    | 'null'
    | 'boolean'
    | 'number'
    | 'text'
    | 'json'
    | 'binary'
    | 'date_time'
    | 'other'
    | 'redacted'
  value: string | null
}

export type DatabaseRow = {
  cells: Record<string, DatabaseCell>
}

export type DatabaseRowPage = {
  table: DatabaseTableRef
  columns: DatabaseColumn[]
  rows: DatabaseRow[]
  next_cursor: string | null
}

export type DatabaseInspectorCapabilities = {
  provider: string
  supports_namespaces: boolean
  supports_exact_count: boolean
  max_page_size: number
}

export function hasScope(
  principal: AdminPrincipal | null,
  scope: string,
): boolean {
  return Boolean(
    principal &&
      (principal.bootstrap ||
        principal.scopes.includes('*') ||
        principal.scopes.includes(scope)),
  )
}
