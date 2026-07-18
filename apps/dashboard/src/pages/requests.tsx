import { RefreshCw } from 'lucide-react'
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
import { HttpStatusBadge } from '@/components/status-badge'
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { RequestTrace } from '@/lib/api'

export function RequestsPage() {
  const { data, refresh } = useAdminQuery<{ data: RequestTrace[] }>(
    'requests',
    10_000,
  )
  const traces = data?.data ?? []

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">Request traces</CardTitle>
          <CardDescription>
            The most recent requests served by the platform, newest first.
          </CardDescription>
        </div>
        <Button variant="outline" size="sm" onClick={() => void refresh()}>
          <RefreshCw data-slot="icon" /> Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {traces.length === 0 ? (
          <p className="text-sm text-muted-foreground">No requests recorded yet.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Method</TableHead>
                <TableHead>Path</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Duration</TableHead>
                <TableHead className="text-right">Time</TableHead>
                <TableHead className="text-right">Request ID</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {traces.map((trace) => (
                <TableRow key={trace.request_id}>
                  <TableCell>
                    <Badge variant="secondary" className="font-mono">
                      {trace.method}
                    </Badge>
                  </TableCell>
                  <TableCell className="max-w-64 truncate font-mono text-xs">
                    {trace.path}
                  </TableCell>
                  <TableCell>
                    <HttpStatusBadge code={trace.status_code} />
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {trace.duration_ms} ms
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {new Date(trace.started_at).toLocaleTimeString()}
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs text-muted-foreground">
                    {trace.request_id.slice(0, 8)}…
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
