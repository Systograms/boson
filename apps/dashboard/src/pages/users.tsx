import { KeyRound, Users } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
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
import type { EndUser, EndUserSession } from '@/lib/api'

function sessionStatus(session: EndUserSession) {
  if (session.revoked_at) return 'revoked'
  if (new Date(session.expires_at) <= new Date()) return 'expired'
  return 'active'
}

export function UsersPage() {
  const users = useAdminQuery<{ data: EndUser[] }>('users')
  const sessions = useAdminQuery<{ data: EndUserSession[] }>('sessions')
  const error = users.error ?? sessions.error

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Users className="size-4 text-muted-foreground" />
          Users
        </CardTitle>
        <CardDescription>
          End-user accounts and refresh sessions from the Identity Admin API.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
        <Tabs defaultValue="users">
          <TabsList>
            <TabsTrigger value="users">
              <Users data-slot="icon" /> Users
            </TabsTrigger>
            <TabsTrigger value="sessions">
              <KeyRound data-slot="icon" /> Sessions
            </TabsTrigger>
          </TabsList>

          <TabsContent value="users" className="mt-4">
            {(users.data?.data ?? []).length === 0 ? (
              <p className="text-sm text-muted-foreground">No users yet.</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Email</TableHead>
                    <TableHead>Display name</TableHead>
                    <TableHead>Verified</TableHead>
                    <TableHead className="text-right">Created</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {(users.data?.data ?? []).map((user) => (
                    <TableRow key={user.id}>
                      <TableCell className="font-medium">{user.email}</TableCell>
                      <TableCell>{user.display_name}</TableCell>
                      <TableCell>
                        <Badge
                          variant={
                            user.email_verified_at ? 'secondary' : 'outline'
                          }
                        >
                          {user.email_verified_at ? 'verified' : 'unverified'}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right text-muted-foreground">
                        {new Date(user.created_at).toLocaleString()}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </TabsContent>

          <TabsContent value="sessions" className="mt-4">
            {(sessions.data?.data ?? []).length === 0 ? (
              <p className="text-sm text-muted-foreground">No sessions yet.</p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>User</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="text-right">Last used</TableHead>
                    <TableHead className="text-right">Expires</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {(sessions.data?.data ?? []).map((session) => {
                    const status = sessionStatus(session)
                    return (
                      <TableRow key={session.id}>
                        <TableCell className="font-mono text-xs">
                          {session.user_id}
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={
                              status === 'active' ? 'secondary' : 'outline'
                            }
                          >
                            {status}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground">
                          {session.last_used_at
                            ? new Date(session.last_used_at).toLocaleString()
                            : '—'}
                        </TableCell>
                        <TableCell className="text-right text-muted-foreground">
                          {new Date(session.expires_at).toLocaleString()}
                        </TableCell>
                      </TableRow>
                    )
                  })}
                </TableBody>
              </Table>
            )}
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
