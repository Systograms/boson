import { Fragment, useMemo, useState } from 'react'
import { ChevronDown, ChevronRight, FileClock, Search } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { AuditEntry } from '@/lib/api'

function topicBadge(topic: string) {
  const capability = topic.split('.')[0]
  return capability ?? topic
}

export function AuditPage() {
  const { data, error } = useAdminQuery<{ data: AuditEntry[] }>('audit', 15_000)
  const entries = useMemo(() => data?.data ?? [], [data])
  const [query, setQuery] = useState('')
  const [expanded, setExpanded] = useState<string | null>(null)

  const filtered = useMemo(() => {
    const normalized = query.trim().toLowerCase()
    if (!normalized) return entries
    return entries.filter(
      (entry) =>
        entry.topic.toLowerCase().includes(normalized) ||
        (entry.correlation_id ?? '').toLowerCase().includes(normalized),
    )
  }, [entries, query])

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <FileClock className="size-4 text-muted-foreground" />
          Audit trail
        </CardTitle>
        <CardDescription>
          Immutable record of every platform event, newest first.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
        <div className="mb-4 flex gap-2">
          <div className="relative min-w-0 flex-1">
            <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter by topic or correlation ID…"
              className="pl-9"
            />
          </div>
          <Badge variant="secondary" className="justify-center tabular-nums">
            {filtered.length} / {entries.length}
          </Badge>
        </div>
        {entries.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No audit entries yet. Actions that emit events appear here once the
            worker records them.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Capability</TableHead>
                <TableHead>Topic</TableHead>
                <TableHead>Correlation</TableHead>
                <TableHead className="text-right">Occurred</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {filtered.map((entry) => {
                const isExpanded = expanded === entry.event_id
                return (
                  <Fragment key={entry.event_id}>
                    <TableRow
                      className="cursor-pointer"
                      onClick={() =>
                        setExpanded(isExpanded ? null : entry.event_id)
                      }
                    >
                      <TableCell>
                        <span className="flex items-center gap-2">
                          {isExpanded ? (
                            <ChevronDown className="size-3.5 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="size-3.5 text-muted-foreground" />
                          )}
                          <Badge variant="secondary">
                            {topicBadge(entry.topic)}
                          </Badge>
                        </span>
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {entry.topic}
                      </TableCell>
                      <TableCell className="font-mono text-xs text-muted-foreground">
                        {entry.correlation_id
                          ? `${entry.correlation_id.slice(0, 8)}…`
                          : '—'}
                      </TableCell>
                      <TableCell className="text-right text-xs text-muted-foreground">
                        {new Date(entry.occurred_at).toLocaleString()}
                      </TableCell>
                    </TableRow>
                    {isExpanded && (
                      <TableRow key={`${entry.event_id}-payload`}>
                        <TableCell colSpan={4} className="bg-muted/30">
                          <pre className="max-h-64 overflow-auto rounded-lg bg-background/80 p-3 font-mono text-xs">
                            {JSON.stringify(entry.payload, null, 2)}
                          </pre>
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
