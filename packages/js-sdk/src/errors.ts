export type BosonErrorBody = {
  error?: {
    code?: string
    message?: string
  } | string
}

export class BosonApiError extends Error {
  readonly status?: number
  readonly code?: string

  constructor(message: string, options: { status?: number; code?: string; cause?: unknown } = {}) {
    super(message, { cause: options.cause })
    this.name = 'BosonApiError'
    this.status = options.status
    this.code = options.code
  }
}

export function parseErrorBody(body: unknown): { code?: string; message?: string } {
  if (!body || typeof body !== 'object' || !('error' in body)) return {}
  const error = (body as BosonErrorBody).error
  if (typeof error === 'string') return { message: error }
  if (!error || typeof error !== 'object') return {}
  return {
    ...(typeof error.code === 'string' ? { code: error.code } : {}),
    ...(typeof error.message === 'string' ? { message: error.message } : {}),
  }
}
