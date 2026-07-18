import { Fragment, useState } from 'react'
import { ChevronDown, ChevronRight, RefreshCw, RotateCcw } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useAdminQuery } from '@/hooks/use-admin-query'
import { adminPost, hasScope, type AdminPrincipal, type JobRecord } from '@/lib/api'

export function JobsPage({ principal }: { principal: AdminPrincipal | null }) {
  const { data, error, loading, refresh } = useAdminQuery<{ data: JobRecord[] }>('jobs', 10_000)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [retrying, setRetrying] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const jobs = data?.data ?? []

  async function retry(id: string) {
    setRetrying(id)
    setActionError(null)
    try {
      await adminPost(`jobs/${id}/retry`, {})
      await refresh()
    } catch (reason) {
      setActionError(reason instanceof Error ? reason.message : 'Retry failed')
    } finally {
      setRetrying(null)
    }
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">Jobs</CardTitle>
          <CardDescription>Durable queue leases and execution outcomes.</CardDescription>
        </div>
        <Button variant="outline" size="sm" disabled={loading} onClick={() => void refresh()}>
          <RefreshCw data-slot="icon" className={loading ? 'animate-spin' : undefined} /> Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {(error || actionError) && <p className="mb-4 text-sm text-destructive">{actionError ?? error}</p>}
        {jobs.length === 0 ? (
          <p className="text-sm text-muted-foreground">No jobs have been enqueued.</p>
        ) : (
          <Table>
            <TableHeader><TableRow>
              <TableHead>Topic</TableHead><TableHead>Status</TableHead>
              <TableHead>Attempts</TableHead><TableHead>Run at</TableHead>
              <TableHead>Error</TableHead><TableHead className="text-right">Action</TableHead>
            </TableRow></TableHeader>
            <TableBody>
              {jobs.map((job) => {
                const open = expanded === job.envelope.id
                const retryable = job.status === 'failed' || job.status === 'dead'
                return <Fragment key={job.envelope.id}>
                  <TableRow className="cursor-pointer" onClick={() => setExpanded(open ? null : job.envelope.id)}>
                    <TableCell className="font-mono text-xs">
                      <span className="flex items-center gap-2">{open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}{job.envelope.topic}</span>
                    </TableCell>
                    <TableCell><Badge variant={job.status === 'dead' ? 'destructive' : 'secondary'}>{job.status}</Badge></TableCell>
                    <TableCell className="tabular-nums">{job.envelope.attempts} / {job.envelope.max_attempts}</TableCell>
                    <TableCell className="text-xs">{new Date(job.run_at).toLocaleString()}</TableCell>
                    <TableCell className="max-w-56 truncate text-xs text-muted-foreground">{job.last_error ?? '—'}</TableCell>
                    <TableCell className="text-right">
                      <Button variant="outline" size="sm" disabled={!retryable || !hasScope(principal, 'jobs:write') || retrying === job.envelope.id}
                        onClick={(event) => { event.stopPropagation(); void retry(job.envelope.id) }}>
                        <RotateCcw data-slot="icon" /> Retry
                      </Button>
                    </TableCell>
                  </TableRow>
                  {open && <TableRow><TableCell colSpan={6} className="bg-muted/30">
                    <dl className="grid gap-3 text-xs md:grid-cols-3">
                      <div><dt className="text-muted-foreground">Job ID</dt><dd className="break-all font-mono">{job.envelope.id}</dd></div>
                      <div><dt className="text-muted-foreground">Correlation ID</dt><dd className="break-all font-mono">{job.envelope.correlation_id ?? '—'}</dd></div>
                      <div><dt className="text-muted-foreground">Lease</dt><dd>{job.locked_by ? `${job.locked_by} · ${new Date(job.locked_at!).toLocaleString()}` : 'Not leased'}</dd></div>
                    </dl>
                    <pre className="mt-3 overflow-auto rounded-md bg-background p-3 text-xs">{JSON.stringify(job.envelope.payload, null, 2)}</pre>
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
