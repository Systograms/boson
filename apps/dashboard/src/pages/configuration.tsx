import { Lock } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { AdminConfig } from '@/lib/api'

export function ConfigurationPage() {
  const { data } = useAdminQuery<AdminConfig>('config')

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle className="text-base">Effective configuration</CardTitle>
          <CardDescription>
            Secrets are redacted by the server before this ever leaves the API.
          </CardDescription>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="font-mono text-xs">
            {data?.snapshot_id ?? 'snapshot —'}
          </Badge>
          <Badge variant="secondary">
            <Lock data-slot="icon" /> Read-only
          </Badge>
        </div>
      </CardHeader>
      <CardContent>
        <ScrollArea className="h-105 rounded-lg border bg-muted/40">
          <pre className="p-4 font-mono text-xs leading-relaxed">
            {data ? JSON.stringify(data.effective, null, 2) : 'Loading…'}
          </pre>
        </ScrollArea>
      </CardContent>
    </Card>
  )
}
