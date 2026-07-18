import { Bell, RefreshCw } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { NotificationDelivery } from '@/lib/api'

export function NotificationsPage() {
  const deliveries = useAdminQuery<{ data: NotificationDelivery[] }>(
    'notifications',
    15_000,
  )
  const data = deliveries.data?.data ?? []

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="flex items-center gap-2 text-base">
            <Bell className="size-4 text-muted-foreground" />
            Notifications
          </CardTitle>
          <CardDescription>
            Provider-neutral email delivery status. Message bodies and action
            tokens are never retained here.
          </CardDescription>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={deliveries.loading}
          onClick={() => void deliveries.refresh()}
        >
          <RefreshCw
            data-slot="icon"
            className={deliveries.loading ? 'animate-spin' : undefined}
          />
          Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {deliveries.error && (
          <p className="mb-4 text-sm text-destructive">{deliveries.error}</p>
        )}
        {data.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No notification deliveries yet.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Kind</TableHead>
                <TableHead>Recipient</TableHead>
                <TableHead>Subject</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Attempts</TableHead>
                <TableHead className="text-right">Last attempt</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {data.map((delivery) => (
                <TableRow key={delivery.event_id}>
                  <TableCell>
                    <Badge variant="secondary">
                      {delivery.kind.replaceAll('_', ' ')}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-medium">
                    {delivery.recipient}
                  </TableCell>
                  <TableCell>{delivery.subject}</TableCell>
                  <TableCell>
                    <Badge
                      variant={
                        delivery.status === 'failed'
                          ? 'destructive'
                          : delivery.status === 'sent'
                            ? 'secondary'
                            : 'outline'
                      }
                    >
                      {delivery.status}
                    </Badge>
                    {delivery.last_error && (
                      <p className="mt-1 max-w-64 truncate text-xs text-destructive">
                        {delivery.last_error}
                      </p>
                    )}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {delivery.attempts}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {delivery.last_attempted_at
                      ? new Date(delivery.last_attempted_at).toLocaleString()
                      : '—'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
