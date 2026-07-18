import type { RequestTrace } from '@/lib/api'

export type TrafficBucket = {
  start: number
  label: string
  ok: number
  errors: number
  p50: number | null
  p95: number | null
}

export type StatusKey = '2xx' | '3xx' | '4xx' | '5xx' | 'other'

export type StatusSlice = {
  key: StatusKey
  count: number
}

function percentile(sorted: number[], q: number): number | null {
  if (sorted.length === 0) return null
  const index = Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)
  return sorted[Math.max(0, index)]
}

/**
 * Slice the retained traces into fixed time buckets ending at "now", so the
 * charts always cover the window the server's in-memory buffer holds.
 */
export function bucketTraces(
  traces: RequestTrace[],
  bucketCount = 24,
): TrafficBucket[] {
  if (traces.length === 0) return []

  const now = Date.now()
  const oldest = traces.reduce(
    (min, trace) => Math.min(min, Date.parse(trace.started_at)),
    now,
  )
  const spanMs = Math.max(now - oldest, 60_000)
  const bucketMs = Math.ceil(spanMs / bucketCount)
  const firstStart = now - bucketMs * bucketCount
  const showSeconds = bucketMs < 60_000

  const durations: number[][] = Array.from({ length: bucketCount }, () => [])
  const buckets: TrafficBucket[] = Array.from(
    { length: bucketCount },
    (_, index) => {
      const start = firstStart + index * bucketMs
      return {
        start,
        label: new Date(start).toLocaleTimeString([], {
          hour: '2-digit',
          minute: '2-digit',
          second: showSeconds ? '2-digit' : undefined,
        }),
        ok: 0,
        errors: 0,
        p50: null,
        p95: null,
      }
    },
  )

  for (const trace of traces) {
    const index = Math.floor(
      (Date.parse(trace.started_at) - firstStart) / bucketMs,
    )
    if (index < 0 || index >= bucketCount) continue
    if (trace.status_code >= 500) buckets[index].errors += 1
    else buckets[index].ok += 1
    durations[index].push(trace.duration_ms)
  }

  buckets.forEach((bucket, index) => {
    const sorted = durations[index].toSorted((a, b) => a - b)
    bucket.p50 = percentile(sorted, 0.5)
    bucket.p95 = percentile(sorted, 0.95)
  })

  return buckets
}

export function statusBreakdown(traces: RequestTrace[]): StatusSlice[] {
  const counts: Record<StatusKey, number> = {
    '2xx': 0,
    '3xx': 0,
    '4xx': 0,
    '5xx': 0,
    other: 0,
  }
  for (const trace of traces) {
    if (trace.status_code >= 200 && trace.status_code < 300) counts['2xx'] += 1
    else if (trace.status_code < 400) counts['3xx'] += 1
    else if (trace.status_code < 500) counts['4xx'] += 1
    else if (trace.status_code < 600) counts['5xx'] += 1
    else counts.other += 1
  }
  return (Object.entries(counts) as Array<[StatusKey, number]>)
    .filter(([, count]) => count > 0)
    .map(([key, count]) => ({ key, count }))
}

/** Human label for the total span covered by the buckets, e.g. "14m". */
export function windowLabel(buckets: TrafficBucket[]): string | null {
  if (buckets.length === 0) return null
  const spanSeconds = Math.round((Date.now() - buckets[0].start) / 1000)
  if (spanSeconds < 90) return `${spanSeconds}s`
  if (spanSeconds < 90 * 60) return `${Math.round(spanSeconds / 60)}m`
  return `${(spanSeconds / 3600).toFixed(1)}h`
}
