import { Activity, AlertTriangle, Cpu, History } from 'lucide-react'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { StatusBadge } from '@/components/status-badge'
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { Overview } from '@/lib/api'

function timeAgo(iso: string): string {
  const seconds = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000))
  if (seconds < 60) return `${seconds}s ago`
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`
  return `${Math.round(seconds / 3600)}h ago`
}

export function OverviewPage() {
  const { data, loading } = useAdminQuery<Overview>('overview', 10_000)

  const metrics = [
    {
      title: 'Total requests',
      icon: Activity,
      value: data?.metrics.total_requests.toLocaleString(),
      hint: 'Since server start',
    },
    {
      title: 'Errors',
      icon: AlertTriangle,
      value: data?.metrics.total_errors.toLocaleString(),
      hint: 'HTTP 5xx responses',
    },
    {
      title: 'Error rate',
      icon: AlertTriangle,
      value:
        data === null
          ? undefined
          : `${(data.metrics.error_rate * 100).toFixed(2)}%`,
      hint: 'Errors / requests',
    },
    {
      title: 'Retained traces',
      icon: History,
      value: data?.metrics.retained_traces.toLocaleString(),
      hint: 'In-memory request traces',
    },
  ]

  return (
    <div className="grid gap-4">
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {metrics.map((metric) => (
          <Card key={metric.title}>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium">{metric.title}</CardTitle>
              <metric.icon className="size-4 text-muted-foreground" />
            </CardHeader>
            <CardContent>
              {loading && metric.value === undefined ? (
                <Skeleton className="h-7 w-20" />
              ) : (
                <p className="text-2xl font-semibold tabular-nums">
                  {metric.value ?? '—'}
                </p>
              )}
              <p className="mt-1 text-xs text-muted-foreground">{metric.hint}</p>
            </CardContent>
          </Card>
        ))}
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Cpu className="size-4 text-muted-foreground" /> Workers
          </CardTitle>
          <CardDescription>
            Background workers reporting heartbeats through PostgreSQL.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {data && data.workers.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No workers have reported yet. Start <code>boson-worker</code> with
              PostgreSQL enabled.
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Last heartbeat</TableHead>
                  <TableHead className="text-right">Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(data?.workers ?? []).map((worker) => {
                  const fresh =
                    Date.now() - Date.parse(worker.last_heartbeat) < 30_000
                  return (
                    <TableRow key={worker.name}>
                      <TableCell className="font-medium">{worker.name}</TableCell>
                      <TableCell className="text-muted-foreground">
                        {timeAgo(worker.last_heartbeat)}
                      </TableCell>
                      <TableCell className="text-right">
                        <StatusBadge status={fresh ? 'ok' : 'down'} />
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
