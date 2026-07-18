import { Fragment, useState } from 'react'
import { ChevronDown, ChevronRight, RefreshCw } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useAdminQuery } from '@/hooks/use-admin-query'
import { adminGet, type EventDetail, type EventRecord } from '@/lib/api'

export function EventsPage() {
  const { data, error, loading, refresh } = useAdminQuery<{ data: EventRecord[] }>('events', 10_000)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [detail, setDetail] = useState<EventDetail | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)
  const events = data?.data ?? []

  async function toggle(id: string) {
    if (expanded === id) {
      setExpanded(null)
      return
    }
    setExpanded(id)
    setDetail(null)
    setDetailError(null)
    try {
      const response = await adminGet<{ data: EventDetail }>(`events/${id}`)
      setDetail(response.data)
    } catch (reason) {
      setDetailError(reason instanceof Error ? reason.message : 'Unable to load event')
    }
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">Events</CardTitle>
          <CardDescription>Transactional outbox state and consumer deliveries.</CardDescription>
        </div>
        <Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>
          <RefreshCw data-slot="icon" className={loading ? 'animate-spin' : undefined} /> Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
        {events.length === 0 ? (
          <p className="text-sm text-muted-foreground">No events have been published.</p>
        ) : (
          <Table>
            <TableHeader><TableRow>
              <TableHead>Topic</TableHead><TableHead>Status</TableHead>
              <TableHead>Time</TableHead><TableHead>Correlation</TableHead>
              <TableHead className="text-right">Attempts</TableHead>
            </TableRow></TableHeader>
            <TableBody>
              {events.map((event) => {
                const open = expanded === event.id
                return <Fragment key={event.id}>
                  <TableRow className="cursor-pointer" onClick={() => void toggle(event.id)}>
                    <TableCell className="font-mono text-xs">
                      <span className="flex items-center gap-2">{open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}{event.topic}</span>
                    </TableCell>
                    <TableCell><Badge variant={event.status === 'pending' ? 'outline' : 'secondary'}>{event.status}</Badge></TableCell>
                    <TableCell className="text-xs">{new Date(event.occurred_at).toLocaleString()}</TableCell>
                    <TableCell className="max-w-48 truncate font-mono text-xs">{event.correlation_id ?? '—'}</TableCell>
                    <TableCell className="text-right tabular-nums">{event.attempts}</TableCell>
                  </TableRow>
                  {open && <TableRow><TableCell colSpan={5} className="bg-muted/30">
                    {detailError ? <p className="text-sm text-destructive">{detailError}</p> : !detail ? (
                      <p className="text-sm text-muted-foreground">Loading event detail…</p>
                    ) : <>
                      <div className="mb-3">
                        <p className="mb-1 text-xs font-medium">Payload</p>
                        <pre className="overflow-auto rounded-md bg-background p-3 text-xs">{JSON.stringify(detail.event.payload, null, 2)}</pre>
                      </div>
                      <p className="mb-1 text-xs font-medium">Deliveries</p>
                      {detail.deliveries.length === 0 ? <p className="text-xs text-muted-foreground">No consumers registered for this topic.</p> :
                        <div className="space-y-2">{detail.deliveries.map((delivery) =>
                          <div key={delivery.consumer} className="rounded-md border bg-background p-3 text-xs">
                            <div className="flex items-center justify-between"><span className="font-mono">{delivery.consumer}</span><Badge variant={delivery.status === 'failed' ? 'destructive' : 'secondary'}>{delivery.status}</Badge></div>
                            <p className="mt-1 text-muted-foreground">Attempts: {delivery.attempts}{delivery.delivered_at ? ` · Delivered ${new Date(delivery.delivered_at).toLocaleString()}` : ''}</p>
                            {delivery.last_error && <p className="mt-1 text-destructive">{delivery.last_error}</p>}
                          </div>)}</div>}
                    </>}
                  </TableCell></TableRow>}
                </Fragment>
              })}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
