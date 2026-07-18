import { ArchiveX } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
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
import { useAdminQuery } from '@/hooks/use-admin-query'
import type { AdminFile } from '@/lib/api'

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function StoragePage() {
  const files = useAdminQuery<{ data: AdminFile[] }>('files')

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <ArchiveX className="size-4 text-muted-foreground" />
          Storage
        </CardTitle>
        <CardDescription>
          File metadata from the Files Admin API. Object keys stay private to
          the capability.
        </CardDescription>
      </CardHeader>
      <CardContent>
        {files.error && (
          <p className="mb-4 text-sm text-destructive">{files.error}</p>
        )}
        {(files.data?.data ?? []).length === 0 ? (
          <p className="text-sm text-muted-foreground">No files uploaded yet.</p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Filename</TableHead>
                <TableHead>Owner</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="text-right">Size</TableHead>
                <TableHead className="text-right">Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(files.data?.data ?? []).map((file) => (
                <TableRow key={file.id}>
                  <TableCell className="max-w-48 truncate font-medium">
                    {file.filename}
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {file.owner_id.slice(0, 8)}…
                  </TableCell>
                  <TableCell className="max-w-36 truncate text-xs text-muted-foreground">
                    {file.content_type}
                  </TableCell>
                  <TableCell>
                    <Badge
                      variant={
                        file.deleted_at || file.status === 'deleted'
                          ? 'outline'
                          : 'secondary'
                      }
                    >
                      {file.deleted_at ? 'deleted' : file.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">
                    {formatBytes(file.size_bytes)}
                  </TableCell>
                  <TableCell className="text-right text-muted-foreground">
                    {new Date(file.created_at).toLocaleString()}
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
