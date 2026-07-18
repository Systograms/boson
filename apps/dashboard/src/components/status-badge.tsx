import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

export function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    ok: 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
    ready: 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-400',
    degraded: 'border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400',
    disabled: 'border-transparent bg-muted text-muted-foreground',
    down: 'border-transparent bg-destructive/15 text-destructive',
  }
  return (
    <Badge variant="outline" className={cn('uppercase', styles[status] ?? '')}>
      {status}
    </Badge>
  )
}

export function HttpStatusBadge({ code }: { code: number }) {
  const tone =
    code >= 500
      ? 'border-transparent bg-destructive/15 text-destructive'
      : code >= 400
        ? 'border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400'
        : 'border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
  return (
    <Badge variant="outline" className={cn('font-mono', tone)}>
      {code}
    </Badge>
  )
}
