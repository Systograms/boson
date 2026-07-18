import type { BosonClient } from './client.js'

export type User = {
  id: string
  email: string
  display_name: string
  email_verified_at: string | null
  created_at: string
}

export type AuthTokens = {
  user: User
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
}

export type RegisterInput = {
  email: string
  display_name: string
  password: string
}

export type LoginInput = {
  email: string
  password: string
}

export class AuthClient {
  constructor(private readonly client: BosonClient) {}

  async register(input: RegisterInput): Promise<AuthTokens> {
    return this.withSavedToken(this.post<AuthTokens>('register', input, false))
  }

  async login(input: LoginInput): Promise<AuthTokens> {
    return this.withSavedToken(this.post<AuthTokens>('login', input, false))
  }

  async refresh(refreshToken: string): Promise<AuthTokens> {
    return this.withSavedToken(
      this.post<AuthTokens>('refresh', { refresh_token: refreshToken }, false),
    )
  }

  me(): Promise<User> {
    return this.client.request<User>('/v1/auth/me')
  }

  logout(refreshToken: string): Promise<void> {
    return this.post<void>('logout', { refresh_token: refreshToken }, false)
  }

  requestEmailVerification(): Promise<void> {
    return this.post<void>('email-verification/request', undefined, true)
  }

  verifyEmail(token: string): Promise<void> {
    return this.post<void>('email-verification/confirm', { token }, false)
  }

  requestPasswordReset(email: string): Promise<void> {
    return this.post<void>('password-reset/request', { email }, false)
  }

  confirmPasswordReset(token: string, password: string): Promise<void> {
    return this.post<void>('password-reset/confirm', { token, password }, false)
  }

  private post<T>(path: string, body: unknown, authenticated: boolean): Promise<T> {
    return this.client.request<T>(
      `/v1/auth/${path}`,
      {
        method: 'POST',
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      },
      { authenticated },
    )
  }

  private async withSavedToken(request: Promise<AuthTokens>): Promise<AuthTokens> {
    const tokens = await request
    await this.client.saveToken(tokens.access_token)
    return tokens
  }
}
