# @boson/sdk

Typed, dependency-free HTTP client for Boson. It targets modern browsers and Node.js 22 or later.

```ts
import { createClient } from '@boson/sdk'

const boson = createClient({
  baseUrl: 'http://localhost:8080',
  getToken: () => localStorage.getItem('boson.token'),
  setToken: (token) => localStorage.setItem('boson.token', token),
})

const session = await boson.auth.login({
  email: 'person@example.com',
  password: 'correct-horse-battery',
})

const users = await boson.admin.get<{ data: unknown[] }>('users')
```

Authentication methods mirror the routes under `/v1/auth`. `admin.get` and `admin.post` provide typed access to application and built-in routes under `/admin/v1` without inventing a capability-authoring layer.

API failures throw `BosonApiError`, which exposes the HTTP `status` and Boson error `code` when present.
