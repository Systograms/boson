import { Fragment, useEffect, useMemo, useState } from 'react'
import {
  ChevronDown,
  ChevronRight,
  RefreshCw,
  Search,
  Workflow,
  ListTree,
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
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
import { adminGet, type RequestDetail, type RequestTrace } from '@/lib/api'

export function RequestsPage() {
  const { data, loading, refresh, updatedAt } = useAdminQuery<{
    data: RequestTrace[]
  }>('requests', 10_000)
  const traces = useMemo(() => data?.data ?? [], [data])
  const [query, setQuery] = useState('')
  const [method, setMethod] = useState('all')
  const [status, setStatus] = useState('all')
  const [expanded, setExpanded] = useState<string | null>(null)
  const [details, setDetails] = useState<Record<string, RequestDetail>>({})
  const [detailLoading, setDetailLoading] = useState<string | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)

  const methods = useMemo(
    () => [...new Set(traces.map((trace) => trace.method))].sort(),
    [traces],
  )
  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    return traces.filter((trace) => {
      const matchesQuery =
        !normalized ||
        trace.path.toLowerCase().includes(normalized) ||
        trace.request_id.toLowerCase().includes(normalized)
      const matchesMethod = method === 'all' || trace.method === method
      const statusClass = Math.floor(trace.status_code / 100)
      const matchesStatus =
        status === 'all' || statusClass.toString() === status
      return matchesQuery && matchesMethod && matchesStatus
    })
  }, [method, query, status, traces])

  useEffect(() => {
    if (!expanded || details[expanded]) return
    let cancelled = false
    setDetailLoading(expanded)
    setDetailError(null)
    void adminGet<{ data: RequestDetail }>(`requests/${expanded}`)
      .then((response) => {
        if (cancelled) return
        setDetails((previous) => ({
          ...previous,
          [expanded]: response.data,
        }))
      })
      .catch((reason: unknown) => {
        if (cancelled) return
        setDetailError(
          reason instanceof Error ? reason.message : 'Failed to load detail',
        )
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(null)
      })
    return () => {
      cancelled = true
    }
  }, [details, expanded])

  function durationClass(duration: number): string {
    if (duration >= 1000) return 'text-destructive'
    if (duration >= 300) return 'text-amber-600 dark:text-amber-400'
    return 'text-emerald-600 dark:text-emerald-400'
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">Request traces</CardTitle>
          <CardDescription>
            Durable request history with correlated outbox events and jobs.
          </CardDescription>
        </div>
        <div className="flex items-center gap-3">
          <span className="hidden items-center gap-1.5 text-xs text-muted-foreground sm:flex">
            <span
              className={`size-2 rounded-full bg-emerald-500 ${loading ? 'animate-pulse' : ''}`}
            />
            Live · 10s
            {updatedAt && (
              <span className="tabular-nums">
                · {new Date(updatedAt).toLocaleTimeString()}
              </span>
            )}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={loading}
            onClick={() => void refresh()}
          >
            <RefreshCw
              data-slot="icon"
              className={loading ? 'animate-spin' : undefined}
            />{' '}
            Refresh
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <div className="mb-4 flex flex-col gap-2 sm:flex-row">
          <div className="relative min-w-0 flex-1">
            <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter by path or request ID…"
              className="pl-9"
            />
          </div>
          <Select value={method} onValueChange={setMethod}>
            <SelectTrigger aria-label="Filter by method">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All methods</SelectItem>
              {methods.map((item) => (
                <SelectItem key={item} value={item}>
                  {item}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={status} onValueChange={setStatus}>
            <SelectTrigger aria-label="Filter by status">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All statuses</SelectItem>
              <SelectItem value="2">2xx success</SelectItem>
              <SelectItem value="3">3xx redirect</SelectItem>
              <SelectItem value="4">4xx client error</SelectItem>
              <SelectItem value="5">5xx server error</SelectItem>
            </SelectContent>
          </Select>
          <Badge variant="secondary" className="justify-center tabular-nums">
            {filtered.length} / {traces.length}
          </Badge>
        </div>
        {traces.length === 0 ? (
          <p className="text-sm text-muted-foreground">No requests recorded yet.</p>
        ) : filtered.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">
            No traces match these filters.
          </p>
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
              {filtered.map((trace) => {
                const isExpanded = expanded === trace.request_id
                const detail = details[trace.request_id]
                return (
                  <Fragment key={trace.request_id}>
                    <TableRow
                      className="cursor-pointer"
                      onClick={() =>
                        setExpanded(isExpanded ? null : trace.request_id)
                      }
                    >
                      <TableCell>
                        <span className="flex items-center gap-2">
                          {isExpanded ? (
                            <ChevronDown className="size-3.5 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="size-3.5 text-muted-foreground" />
                          )}
                          <Badge variant="secondary" className="font-mono">
                            {trace.method}
                          </Badge>
                        </span>
                      </TableCell>
                      <TableCell className="max-w-64 truncate font-mono text-xs">
                        {trace.path}
                      </TableCell>
                      <TableCell>
                        <HttpStatusBadge code={trace.status_code} />
                      </TableCell>
                      <TableCell
                        className={`text-right font-medium tabular-nums ${durationClass(trace.duration_ms)}`}
                      >
                        {trace.duration_ms} ms
                      </TableCell>
                      <TableCell className="text-right text-xs text-muted-foreground">
                        {new Date(trace.started_at).toLocaleTimeString()}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs text-muted-foreground">
                        {trace.request_id.slice(0, 8)}…
                      </TableCell>
                    </TableRow>
                    {isExpanded && (
                      <TableRow key={`${trace.request_id}-details`}>
                        <TableCell colSpan={6} className="bg-muted/30">
                          <div className="space-y-4 py-2">
                            <dl className="grid gap-3 text-xs sm:grid-cols-2 xl:grid-cols-4">
                              <div>
                                <dt className="text-muted-foreground">Full path</dt>
                                <dd className="mt-1 break-all font-mono">
                                  {trace.path}
                                </dd>
                              </div>
                              <div>
                                <dt className="text-muted-foreground">
                                  Request ID
                                </dt>
                                <dd className="mt-1 break-all font-mono">
                                  {trace.request_id}
                                </dd>
                              </div>
                              <div>
                                <dt className="text-muted-foreground">Started</dt>
                                <dd className="mt-1">
                                  {new Date(trace.started_at).toLocaleString()}
                                </dd>
                              </div>
                              <div>
                                <dt className="text-muted-foreground">Duration</dt>
                                <dd className="mt-1 font-medium tabular-nums">
                                  {trace.duration_ms} ms
                                </dd>
                              </div>
                            </dl>

                            {detailLoading === trace.request_id && (
                              <p className="text-xs text-muted-foreground">
                                Loading correlated timeline…
                              </p>
                            )}
                            {detailError &&
                              detailLoading !== trace.request_id &&
                              !detail && (
                                <p className="text-xs text-destructive">
                                  {detailError}
                                </p>
                              )}
                            {detail && (
                              <div className="grid gap-4 lg:grid-cols-2">
                                <div>
                                  <h3 className="mb-2 flex items-center gap-1.5 text-xs font-medium">
                                    <Workflow className="size-3.5 text-muted-foreground" />
                                    Events
                                    <Badge variant="secondary" className="ml-1">
                                      {detail.events.length}
                                    </Badge>
                                  </h3>
                                  {detail.events.length === 0 ? (
                                    <p className="text-xs text-muted-foreground">
                                      No outbox events for this request.
                                    </p>
                                  ) : (
                                    <ul className="space-y-2">
                                      {detail.events.map((event) => (
                                        <li
                                          key={event.id}
                                          className="rounded-lg bg-background/80 px-3 py-2 text-xs"
                                        >
                                          <div className="flex items-center justify-between gap-2">
                                            <span className="font-mono">
                                              {event.topic}
                                            </span>
                                            <Badge variant="outline">
                                              {event.status}
                                            </Badge>
                                          </div>
                                          <p className="mt-1 text-muted-foreground">
                                            {new Date(
                                              event.occurred_at,
                                            ).toLocaleString()}
                                            {event.last_error
                                              ? ` · ${event.last_error}`
                                              : ''}
                                          </p>
                                        </li>
                                      ))}
                                    </ul>
                                  )}
                                </div>
                                <div>
                                  <h3 className="mb-2 flex items-center gap-1.5 text-xs font-medium">
                                    <ListTree className="size-3.5 text-muted-foreground" />
                                    Jobs
                                    <Badge variant="secondary" className="ml-1">
                                      {detail.jobs.length}
                                    </Badge>
                                  </h3>
                                  {detail.jobs.length === 0 ? (
                                    <p className="text-xs text-muted-foreground">
                                      No jobs for this request.
                                    </p>
                                  ) : (
                                    <ul className="space-y-2">
                                      {detail.jobs.map((job) => (
                                        <li
                                          key={job.id}
                                          className="rounded-lg bg-background/80 px-3 py-2 text-xs"
                                        >
                                          <div className="flex items-center justify-between gap-2">
                                            <span className="font-mono">
                                              {job.topic}
                                            </span>
                                            <Badge variant="outline">
                                              {job.status}
                                            </Badge>
                                          </div>
                                          <p className="mt-1 text-muted-foreground">
                                            attempts {job.attempts}
                                            {job.last_error
                                              ? ` · ${job.last_error}`
                                              : ''}
                                          </p>
                                        </li>
                                      ))}
                                    </ul>
                                  )}
                                </div>
                              </div>
                            )}
                          </div>
                        </TableCell>
                      </TableRow>
                    )}
                  </Fragment>
                )
              })}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
