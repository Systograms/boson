import { Building2, Mail, Users } from 'lucide-react'
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
import type {
  Organization,
  OrganizationInvitation,
  OrganizationMembership,
} from '@/lib/api'

function invitationStatus(invitation: OrganizationInvitation) {
  if (invitation.accepted_at) return 'accepted'
  if (invitation.revoked_at) return 'revoked'
  if (new Date(invitation.expires_at) <= new Date()) return 'expired'
  return 'pending'
}

export function OrganizationsPage() {
  const organizations =
    useAdminQuery<{ data: Organization[] }>('organizations')
  const memberships =
    useAdminQuery<{ data: OrganizationMembership[] }>(
      'organization-memberships',
    )
  const invitations =
    useAdminQuery<{ data: OrganizationInvitation[] }>(
      'organization-invitations',
    )
  const error =
    organizations.error ?? memberships.error ?? invitations.error

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Building2 className="size-4 text-muted-foreground" />
          Organizations
        </CardTitle>
        <CardDescription>
          Organization directory, role memberships, and invitation lifecycle
          from the Admin API.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
        <Tabs defaultValue="organizations">
          <TabsList>
            <TabsTrigger value="organizations">
              <Building2 data-slot="icon" /> Organizations
            </TabsTrigger>
            <TabsTrigger value="memberships">
              <Users data-slot="icon" /> Memberships
            </TabsTrigger>
            <TabsTrigger value="invitations">
              <Mail data-slot="icon" /> Invitations
            </TabsTrigger>
          </TabsList>

          <TabsContent value="organizations" className="mt-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Slug</TableHead>
                  <TableHead>Created by</TableHead>
                  <TableHead className="text-right">Members</TableHead>
                  <TableHead className="text-right">Created</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(organizations.data?.data ?? []).map((organization) => (
                  <TableRow key={organization.id}>
                    <TableCell className="font-medium">
                      {organization.name}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {organization.slug}
                    </TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {organization.created_by}
                    </TableCell>
                    <TableCell className="text-right">
                      <Badge variant="secondary">
                        {organization.member_count}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {new Date(organization.created_at).toLocaleString()}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TabsContent>

          <TabsContent value="memberships" className="mt-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Organization</TableHead>
                  <TableHead>User</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead className="text-right">Joined</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(memberships.data?.data ?? []).map((membership) => (
                  <TableRow
                    key={`${membership.organization_id}:${membership.user_id}`}
                  >
                    <TableCell className="font-mono text-xs">
                      {membership.organization_id}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {membership.user_id}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{membership.role}</Badge>
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {new Date(membership.joined_at).toLocaleString()}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TabsContent>

          <TabsContent value="invitations" className="mt-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Email</TableHead>
                  <TableHead>Organization</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="text-right">Expires</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(invitations.data?.data ?? []).map((invitation) => (
                  <TableRow key={invitation.id}>
                    <TableCell className="font-medium">
                      {invitation.email}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {invitation.organization_id}
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline">{invitation.role}</Badge>
                    </TableCell>
                    <TableCell>
                      <Badge variant="secondary">
                        {invitationStatus(invitation)}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right text-muted-foreground">
                      {new Date(invitation.expires_at).toLocaleString()}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  )
}
