import { useState, type FormEvent } from 'react'
import {
  Check,
  Copy,
  KeyRound,
  Plus,
  ShieldCheck,
  TriangleAlert,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useAdminQuery } from '@/hooks/use-admin-query'
import {
  adminPost,
  hasScope,
  type AdminApiKey,
  type AdminPrincipal,
  type AdminUser,
  type IssuedApiKey,
  type IssuedCredential,
} from '@/lib/api'

const AVAILABLE_SCOPES = [
  'admins:read',
  'admins:write',
  'audit:read',
  'identity:read',
  'organizations:read',
  'ops:read',
  'config:read',
  'database:read',
  'events:read',
  'jobs:read',
  'jobs:write',
  'notifications:read',
]

type AdminAction =
  | { type: 'create' }
  | { type: 'key'; admin: AdminUser }

export function AdministratorsPage({
  principal,
}: {
  principal: AdminPrincipal | null
}) {
  const admins = useAdminQuery<{ data: AdminUser[] }>('admins')
  const keys = useAdminQuery<{ data: AdminApiKey[] }>('admin-keys')
  const [action, setAction] = useState<AdminAction | null>(null)
  const [displayName, setDisplayName] = useState('')
  const [email, setEmail] = useState('')
  const [keyName, setKeyName] = useState('default')
  const [scopes, setScopes] = useState<string[]>(AVAILABLE_SCOPES)
  const [issuedToken, setIssuedToken] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [submitting, setSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)
  const canWrite = hasScope(principal, 'admins:write')

  function openAction(next: AdminAction) {
    setAction(next)
    setDisplayName('')
    setEmail('')
    setKeyName(next.type === 'create' ? 'default' : '')
    setScopes(AVAILABLE_SCOPES)
    setIssuedToken(null)
    setCopied(false)
    setSubmitError(null)
  }

  function closeAction() {
    setAction(null)
    setIssuedToken(null)
    setSubmitError(null)
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!action) return
    setSubmitting(true)
    setSubmitError(null)
    try {
      const result =
        action.type === 'create'
          ? await adminPost<IssuedCredential>('admins', {
              display_name: displayName,
              email,
              key_name: keyName || undefined,
            })
          : await adminPost<IssuedApiKey>(
              `admins/${action.admin.id}/keys`,
              { name: keyName, scopes },
            )
      setIssuedToken(result.token)
      await Promise.all([admins.refresh(), keys.refresh()])
    } catch (reason) {
      setSubmitError(
        reason instanceof Error ? reason.message : 'Unable to complete request',
      )
    } finally {
      setSubmitting(false)
    }
  }

  async function copyToken() {
    if (!issuedToken) return
    await navigator.clipboard.writeText(issuedToken)
    setCopied(true)
  }

  return (
    <>
      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div>
            <CardTitle className="flex items-center gap-2 text-base">
              <ShieldCheck className="size-4 text-muted-foreground" />
              Platform administrators
            </CardTitle>
            <CardDescription>
              Persistent operators and hashed API keys. New key values are
              returned exactly once and never stored by Boson.
            </CardDescription>
          </div>
          {canWrite && (
            <Button size="sm" onClick={() => openAction({ type: 'create' })}>
              <Plus data-slot="icon" /> Add administrator
            </Button>
          )}
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="admins">
            <TabsList>
              <TabsTrigger value="admins">Administrators</TabsTrigger>
              <TabsTrigger value="keys">API keys</TabsTrigger>
            </TabsList>
            <TabsContent value="admins" className="mt-4">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Email</TableHead>
                    <TableHead>Created</TableHead>
                    <TableHead className="text-right">Status</TableHead>
                    {canWrite && <TableHead className="w-28" />}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {(admins.data?.data ?? []).map((admin) => (
                    <TableRow key={admin.id}>
                      <TableCell className="font-medium">
                        {admin.display_name}
                      </TableCell>
                      <TableCell>{admin.email}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {new Date(admin.created_at).toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right">
                        <Badge
                          variant={
                            admin.disabled_at ? 'destructive' : 'secondary'
                          }
                        >
                          {admin.disabled_at ? 'disabled' : 'active'}
                        </Badge>
                      </TableCell>
                      {canWrite && (
                        <TableCell className="text-right">
                          <Button
                            variant="outline"
                            size="xs"
                            disabled={Boolean(admin.disabled_at)}
                            onClick={() => openAction({ type: 'key', admin })}
                          >
                            <KeyRound data-slot="icon" /> Issue key
                          </Button>
                        </TableCell>
                      )}
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TabsContent>
            <TabsContent value="keys" className="mt-4">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Administrator</TableHead>
                    <TableHead>Prefix</TableHead>
                    <TableHead>Scopes</TableHead>
                    <TableHead className="text-right">Last used</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {(keys.data?.data ?? []).map((key) => {
                    const owner = admins.data?.data.find(
                      (admin) => admin.id === key.admin_id,
                    )
                    return (
                      <TableRow key={key.id}>
                        <TableCell className="font-medium">
                          <span className="flex items-center gap-2">
                            <KeyRound className="size-3.5 text-muted-foreground" />
                            {key.name}
                          </span>
                        </TableCell>
                        <TableCell className="text-muted-foreground">
                          {owner?.display_name ?? 'Unknown'}
                        </TableCell>
                        <TableCell className="font-mono text-xs">
                          {key.token_prefix}…
                        </TableCell>
                        <TableCell>
                          <div className="flex max-w-72 flex-wrap gap-1">
                            {key.scopes.map((scope) => (
                              <Badge
                                key={scope}
                                variant="outline"
                                className="font-mono text-[10px]"
                              >
                                {scope}
                              </Badge>
                            ))}
                          </div>
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground">
                          {key.last_used_at
                            ? new Date(key.last_used_at).toLocaleString()
                            : 'Never'}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>

      <Sheet open={action !== null} onOpenChange={(open) => !open && closeAction()}>
        <SheetContent className="sm:max-w-md">
          <SheetHeader>
            <SheetTitle>
              {issuedToken
                ? 'Save this API key'
                : action?.type === 'create'
                  ? 'Add administrator'
                  : 'Issue API key'}
            </SheetTitle>
            <SheetDescription>
              {issuedToken
                ? 'Boson stores only the token hash. This value cannot be shown again.'
                : action?.type === 'create'
                  ? 'Create a persistent operator and their first API key.'
                  : `Create a scoped key for ${action?.type === 'key' ? action.admin.display_name : ''}.`}
            </SheetDescription>
          </SheetHeader>

          {issuedToken ? (
            <div className="grid gap-4 px-6">
              <div className="flex gap-2 rounded-2xl border border-amber-500/30 bg-amber-500/10 p-3 text-amber-700 dark:text-amber-300">
                <TriangleAlert className="mt-0.5 size-4 shrink-0" />
                <p className="text-xs">
                  Copy this token now and store it securely. Closing this panel
                  permanently hides it.
                </p>
              </div>
              <div className="rounded-2xl bg-muted p-3">
                <code className="break-all font-mono text-xs">{issuedToken}</code>
              </div>
              <Button type="button" onClick={() => void copyToken()}>
                {copied ? <Check data-slot="icon" /> : <Copy data-slot="icon" />}
                {copied ? 'Copied' : 'Copy API key'}
              </Button>
            </div>
          ) : (
            <form className="grid gap-5 px-6" onSubmit={(event) => void submit(event)}>
              {action?.type === 'create' && (
                <>
                  <label className="grid gap-1.5 text-sm font-medium">
                    Display name
                    <Input
                      required
                      autoFocus
                      value={displayName}
                      onChange={(event) => setDisplayName(event.target.value)}
                      placeholder="Platform operator"
                    />
                  </label>
                  <label className="grid gap-1.5 text-sm font-medium">
                    Email
                    <Input
                      required
                      type="email"
                      value={email}
                      onChange={(event) => setEmail(event.target.value)}
                      placeholder="operator@example.com"
                    />
                  </label>
                </>
              )}
              <label className="grid gap-1.5 text-sm font-medium">
                Key name
                <Input
                  required
                  autoFocus={action?.type === 'key'}
                  value={keyName}
                  onChange={(event) => setKeyName(event.target.value)}
                  placeholder="production-cli"
                />
              </label>
              {action?.type === 'key' && (
                <fieldset className="grid gap-2">
                  <legend className="mb-1 text-sm font-medium">Scopes</legend>
                  <div className="grid grid-cols-2 gap-2">
                    {AVAILABLE_SCOPES.map((scope) => (
                      <label
                        key={scope}
                        className="flex items-center gap-2 rounded-xl border px-3 py-2 text-xs"
                      >
                        <input
                          type="checkbox"
                          checked={scopes.includes(scope)}
                          onChange={(event) =>
                            setScopes((current) =>
                              event.target.checked
                                ? [...current, scope]
                                : current.filter((item) => item !== scope),
                            )
                          }
                          className="accent-primary"
                        />
                        <span className="font-mono">{scope}</span>
                      </label>
                    ))}
                  </div>
                </fieldset>
              )}
              {submitError && (
                <p className="text-sm text-destructive">{submitError}</p>
              )}
              <Button type="submit" disabled={submitting}>
                {submitting
                  ? 'Creating…'
                  : action?.type === 'create'
                    ? 'Create administrator'
                    : 'Issue API key'}
              </Button>
            </form>
          )}
          {issuedToken && (
            <SheetFooter>
              <Button variant="outline" onClick={closeAction}>
                I saved the key
              </Button>
            </SheetFooter>
          )}
        </SheetContent>
      </Sheet>
    </>
  )
}
