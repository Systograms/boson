import { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
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
import type { Health } from '@/lib/api'

export function HealthPage() {
  const { data, loading, refresh, updatedAt } = useAdminQuery<Health>(
    'health',
    15_000,
  )
  const [history, setHistory] = useState<
    Record<string, Array<{ status: string; at: number }>>
  >({})

  useEffect(() => {
    if (!data || !updatedAt) return
    setHistory((current) => {
      const next = { ...current }
      for (const check of data.checks) {
        const previous = current[check.name] ?? []
        if (previous.at(-1)?.at === updatedAt) continue
        next[check.name] = [
          ...previous,
          { status: check.status, at: updatedAt },
        ].slice(-24)
      }
      return next
    })
  }, [data, updatedAt])

  const healthyChecks =
    data?.checks.filter((check) => check.status === 'ok').length ?? 0

  function historyColor(status: string): string {
    if (status === 'ok') return 'bg-emerald-500'
    if (status === 'degraded') return 'bg-amber-500'
    return 'bg-destructive'
  }

  return (
    <div className="grid gap-4">
      <div className="grid gap-4 sm:grid-cols-3">
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Overall status</CardDescription>
          </CardHeader>
          <CardContent>
            {loading && !data ? (
              <Skeleton className="h-6 w-16" />
            ) : (
              <StatusBadge status={data?.status ?? 'down'} />
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Server version</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="font-mono text-lg font-semibold">
              {data?.version ?? '—'}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardDescription>Healthy checks</CardDescription>
          </CardHeader>
          <CardContent>
            <p className="text-lg font-semibold tabular-nums">
              {data ? `${healthyChecks} / ${data.checks.length}` : '—'}
            </p>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <div>
            <CardTitle className="text-base">Dependencies</CardTitle>
            <CardDescription>
              Live probes with this session's last 24 samples.
            </CardDescription>
          </div>
          <Button variant="outline" size="sm" onClick={() => void refresh()}>
            <RefreshCw data-slot="icon" /> Refresh
          </Button>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Component</TableHead>
                <TableHead>Detail</TableHead>
                <TableHead>Recent history</TableHead>
                <TableHead className="text-right">Status</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(data?.checks ?? []).map((check) => (
                <TableRow key={check.name}>
                  <TableCell className="font-medium capitalize">
                    {check.name}
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    {check.message ?? 'Responding normally'}
                  </TableCell>
                  <TableCell>
                    <div
                      className="flex h-5 min-w-36 items-stretch gap-0.5"
                      aria-label={`Recent status history for ${check.name}`}
                    >
                      {Array.from({
                        length: Math.max(
                          0,
                          24 - (history[check.name]?.length ?? 0),
                        ),
                      }).map((_, index) => (
                        <span
                          key={`empty-${index}`}
                          className="min-w-1 flex-1 rounded-sm bg-muted"
                        />
                      ))}
                      {(history[check.name] ?? []).map((sample) => (
                        <span
                          key={sample.at}
                          title={`${sample.status} · ${new Date(sample.at).toLocaleTimeString()}`}
                          className={`min-w-1 flex-1 rounded-sm ${historyColor(sample.status)}`}
                        />
                      ))}
                    </div>
                  </TableCell>
                  <TableCell className="text-right">
                    <StatusBadge status={check.status} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  )
}
