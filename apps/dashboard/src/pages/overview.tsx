import { Activity, AlertTriangle, ChartPie, Cpu, Gauge, History } from 'lucide-react'
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Line,
  LineChart,
  Pie,
  PieChart,
  ResponsiveContainer,
  XAxis,
  YAxis,
} from 'recharts'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
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
import type { Overview, RequestTrace } from '@/lib/api'
import {
  bucketTraces,
  statusBreakdown,
  windowLabel,
  type StatusKey,
} from '@/lib/metrics'

const trafficConfig: ChartConfig = {
  ok: { label: 'Success', color: 'var(--primary)' },
  errors: { label: 'Errors (5xx)', color: 'var(--destructive)' },
}

const latencyConfig: ChartConfig = {
  p50: { label: 'p50', color: 'var(--chart-2)' },
  p95: { label: 'p95', color: 'var(--primary)' },
}

const statusConfig: ChartConfig = {
  '2xx': { label: '2xx Success', color: 'var(--primary)' },
  '3xx': { label: '3xx Redirect', color: 'var(--chart-2)' },
  '4xx': { label: '4xx Client error', color: 'var(--chart-3)' },
  '5xx': { label: '5xx Server error', color: 'var(--destructive)' },
  other: { label: 'Other', color: 'var(--chart-4)' },
}

function timeAgo(iso: string): string {
  const seconds = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000))
  if (seconds < 60) return `${seconds}s ago`
  if (seconds < 3600) return `${Math.round(seconds / 60)}m ago`
  return `${Math.round(seconds / 3600)}h ago`
}

function Sparkline({ points, color }: { points: number[]; color: string }) {
  const data = points.map((value, index) => ({ index, value }))
  return (
    <div className="mt-2 h-8">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={data} margin={{ top: 2, right: 0, bottom: 0, left: 0 }}>
          <Area
            dataKey="value"
            type="monotone"
            stroke={color}
            strokeWidth={1.5}
            fill={color}
            fillOpacity={0.12}
            isAnimationActive={false}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  )
}

function ChartEmptyState() {
  return (
    <div className="flex h-56 items-center justify-center text-sm text-muted-foreground">
      No traffic recorded yet. Send a few requests to the server to populate
      this chart.
    </div>
  )
}

export function OverviewPage() {
  const { data, loading } = useAdminQuery<Overview>('overview', 10_000)
  const requests = useAdminQuery<{ data: RequestTrace[] }>('requests', 10_000)

  const traces = requests.data?.data ?? []
  const buckets = bucketTraces(traces)
  const statuses = statusBreakdown(traces)
  const statusTotal = statuses.reduce((sum, slice) => sum + slice.count, 0)
  const span = windowLabel(buckets)
  const windowHint = span ? `Last ${span} of retained traces` : 'Retained trace window'

  const metrics = [
    {
      title: 'Total requests',
      icon: Activity,
      value: data?.metrics.total_requests.toLocaleString(),
      hint: 'Since server start',
      spark: buckets.map((bucket) => bucket.ok + bucket.errors),
      sparkColor: 'var(--primary)',
    },
    {
      title: 'Errors',
      icon: AlertTriangle,
      value: data?.metrics.total_errors.toLocaleString(),
      hint: 'HTTP 5xx responses',
      spark: buckets.map((bucket) => bucket.errors),
      sparkColor: 'var(--destructive)',
    },
    {
      title: 'Error rate',
      icon: Gauge,
      value:
        data === null
          ? undefined
          : `${(data.metrics.error_rate * 100).toFixed(2)}%`,
      hint: 'Errors / requests',
      spark: buckets.map((bucket) => {
        const total = bucket.ok + bucket.errors
        return total === 0 ? 0 : bucket.errors / total
      }),
      sparkColor: 'var(--chart-3)',
    },
    {
      title: 'Retained traces',
      icon: History,
      value: data?.metrics.retained_traces.toLocaleString(),
      hint: 'In-memory request traces',
      spark: null,
      sparkColor: '',
    },
  ]

  return (
    <div className="grid gap-4">
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {metrics.map((metric) => (
          <Card key={metric.title} size="sm">
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
              {metric.spark && metric.spark.some((value) => value > 0) && (
                <Sparkline points={metric.spark} color={metric.sparkColor} />
              )}
            </CardContent>
          </Card>
        ))}
      </div>

      <div className="grid gap-4 xl:grid-cols-3">
        <Card className="xl:col-span-2">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Activity className="size-4 text-muted-foreground" /> Traffic
            </CardTitle>
            <CardDescription>{windowHint}, newest on the right.</CardDescription>
          </CardHeader>
          <CardContent>
            {buckets.length === 0 ? (
              <ChartEmptyState />
            ) : (
              <ChartContainer config={trafficConfig}>
                <BarChart data={buckets} margin={{ left: -16, right: 4 }}>
                  <CartesianGrid vertical={false} />
                  <XAxis
                    dataKey="label"
                    tickLine={false}
                    axisLine={false}
                    minTickGap={32}
                    tickMargin={8}
                  />
                  <YAxis tickLine={false} axisLine={false} allowDecimals={false} />
                  <ChartTooltip
                    cursor={{ fillOpacity: 0.4 }}
                    content={<ChartTooltipContent />}
                  />
                  <Bar
                    dataKey="ok"
                    stackId="traffic"
                    fill="var(--color-ok)"
                    radius={[0, 0, 2, 2]}
                  />
                  <Bar
                    dataKey="errors"
                    stackId="traffic"
                    fill="var(--color-errors)"
                    radius={[2, 2, 0, 0]}
                  />
                </BarChart>
              </ChartContainer>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <ChartPie className="size-4 text-muted-foreground" /> Status codes
            </CardTitle>
            <CardDescription>Responses by status class.</CardDescription>
          </CardHeader>
          <CardContent>
            {statuses.length === 0 ? (
              <ChartEmptyState />
            ) : (
              <div className="grid gap-2">
                <ChartContainer config={statusConfig} className="h-40">
                  <PieChart>
                    <ChartTooltip content={<ChartTooltipContent />} />
                    <Pie
                      data={statuses}
                      dataKey="count"
                      nameKey="key"
                      innerRadius="62%"
                      outerRadius="90%"
                      paddingAngle={2}
                      strokeWidth={0}
                      isAnimationActive={false}
                    >
                      {statuses.map((slice) => (
                        <Cell
                          key={slice.key}
                          fill={statusConfig[slice.key as StatusKey]?.color}
                        />
                      ))}
                    </Pie>
                  </PieChart>
                </ChartContainer>
                <div className="grid gap-1.5">
                  {statuses.map((slice) => (
                    <div
                      key={slice.key}
                      className="flex items-center gap-2 text-xs"
                    >
                      <span
                        className="size-2 shrink-0 rounded-full"
                        style={{ background: statusConfig[slice.key]?.color }}
                      />
                      <span className="text-muted-foreground">
                        {statusConfig[slice.key]?.label}
                      </span>
                      <span className="ml-auto font-medium tabular-nums">
                        {slice.count.toLocaleString()}
                        <span className="ml-1.5 text-muted-foreground">
                          {statusTotal === 0
                            ? ''
                            : `${((slice.count / statusTotal) * 100).toFixed(0)}%`}
                        </span>
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Gauge className="size-4 text-muted-foreground" /> Latency
          </CardTitle>
          <CardDescription>
            Median and 95th percentile response time per interval.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {buckets.length === 0 ? (
            <ChartEmptyState />
          ) : (
            <ChartContainer config={latencyConfig} className="h-48">
              <LineChart data={buckets} margin={{ left: -16, right: 4 }}>
                <CartesianGrid vertical={false} />
                <XAxis
                  dataKey="label"
                  tickLine={false}
                  axisLine={false}
                  minTickGap={32}
                  tickMargin={8}
                />
                <YAxis
                  tickLine={false}
                  axisLine={false}
                  allowDecimals={false}
                  tickFormatter={(value: number) => `${value}ms`}
                />
                <ChartTooltip
                  content={
                    <ChartTooltipContent
                      valueFormatter={(value) => `${value} ms`}
                    />
                  }
                />
                <Line
                  dataKey="p50"
                  type="monotone"
                  stroke="var(--color-p50)"
                  strokeWidth={2}
                  dot={false}
                  connectNulls
                  isAnimationActive={false}
                />
                <Line
                  dataKey="p95"
                  type="monotone"
                  stroke="var(--color-p95)"
                  strokeWidth={2}
                  dot={false}
                  connectNulls
                  isAnimationActive={false}
                />
              </LineChart>
            </ChartContainer>
          )}
        </CardContent>
      </Card>

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
