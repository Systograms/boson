import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from 'react'
import {
  Braces,
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Database,
  Download,
  KeyRound,
  RefreshCw,
  Search,
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
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '@/components/ui/sheet'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAdminQuery } from '@/hooks/use-admin-query'
import {
  adminGet,
  hasScope,
  type AdminPrincipal,
  type DatabaseCell,
  type DatabaseInspectorCapabilities,
  type DatabaseRowPage,
  type DatabaseTableRef,
  type DatabaseTableSchema,
  type DatabaseTableSummary,
} from '@/lib/api'

type AppliedFilter = { column: string; value: string } | null

function tableKey(table: DatabaseTableRef): string {
  return `${table.namespace}.${table.name}`
}

function displayCount(summary: DatabaseTableSummary): string {
  if (!summary.row_count) return '—'
  const prefix = summary.row_count.exact ? '' : '~'
  return `${prefix}${summary.row_count.value.toLocaleString()}`
}

function cellText(cell: DatabaseCell | undefined): string {
  if (!cell || cell.kind === 'null') return 'NULL'
  if (cell.kind === 'redacted') return '[REDACTED]'
  return cell.value ?? ''
}

function prettyJson(value: string): string {
  try {
    return JSON.stringify(JSON.parse(value), null, 2)
  } catch {
    return value
  }
}

function csvValue(value: string): string {
  return `"${value.replaceAll('"', '""')}"`
}

export function DatabasePage({
  principal,
}: {
  principal: AdminPrincipal | null
}) {
  const tablesQuery =
    useAdminQuery<{ data: DatabaseTableSummary[] }>('database/tables')
  const capabilities =
    useAdminQuery<DatabaseInspectorCapabilities>('database')
  const [selected, setSelected] = useState<DatabaseTableRef | null>(null)
  const [schema, setSchema] = useState<DatabaseTableSchema | null>(null)
  const [page, setPage] = useState<DatabaseRowPage | null>(null)
  const [tableSearch, setTableSearch] = useState('')
  const [filterColumn, setFilterColumn] = useState('')
  const [filterValue, setFilterValue] = useState('')
  const [appliedFilter, setAppliedFilter] = useState<AppliedFilter>(null)
  const [cursorHistory, setCursorHistory] = useState<Array<string | null>>([
    null,
  ])
  const [loadingRows, setLoadingRows] = useState(false)
  const [rowError, setRowError] = useState<string | null>(null)
  const [copiedCell, setCopiedCell] = useState<string | null>(null)
  const [jsonCell, setJsonCell] = useState<{
    column: string
    value: string
  } | null>(null)
  const canRead = principal === null || hasScope(principal, 'database:read')
  const cursor = cursorHistory.at(-1) ?? null

  const tables = useMemo(
    () => tablesQuery.data?.data ?? [],
    [tablesQuery.data],
  )
  const visibleTables = useMemo(() => {
    const search = tableSearch.trim().toLowerCase()
    if (!search) return tables
    return tables.filter((summary) =>
      tableKey(summary.table).toLowerCase().includes(search),
    )
  }, [tableSearch, tables])
  const namespaces = useMemo(() => {
    const grouped = new Map<string, DatabaseTableSummary[]>()
    for (const summary of visibleTables) {
      const current = grouped.get(summary.table.namespace) ?? []
      current.push(summary)
      grouped.set(summary.table.namespace, current)
    }
    return [...grouped.entries()]
  }, [visibleTables])

  const chooseTable = useCallback((summary: DatabaseTableSummary) => {
    setSelected(summary.table)
    setSchema(null)
    setPage(null)
    setFilterColumn(summary.primary_key[0] ?? '')
    setFilterValue('')
    setAppliedFilter(null)
    setCursorHistory([null])
    setRowError(null)
  }, [])

  useEffect(() => {
    if (!selected && tables.length > 0) chooseTable(tables[0])
  }, [chooseTable, selected, tables])

  const loadTable = useCallback(async () => {
    if (!selected) return
    setLoadingRows(true)
    setRowError(null)
    const namespace = encodeURIComponent(selected.namespace)
    const name = encodeURIComponent(selected.name)
    const params = new URLSearchParams({ limit: '100' })
    if (cursor) params.set('cursor', cursor)
    if (appliedFilter) {
      params.set('column', appliedFilter.column)
      params.set('value', appliedFilter.value)
    }
    try {
      const [nextSchema, nextPage] = await Promise.all([
        adminGet<DatabaseTableSchema>(
          `database/tables/${namespace}/${name}`,
        ),
        adminGet<DatabaseRowPage>(
          `database/tables/${namespace}/${name}/rows?${params}`,
        ),
      ])
      setSchema(nextSchema)
      setPage(nextPage)
      if (!filterColumn) {
        setFilterColumn(
          nextSchema.primary_key[0] ?? nextSchema.columns[0]?.name ?? '',
        )
      }
    } catch (reason) {
      setRowError(
        reason instanceof Error ? reason.message : 'Unable to inspect table',
      )
    } finally {
      setLoadingRows(false)
    }
  }, [appliedFilter, cursor, filterColumn, selected])

  useEffect(() => {
    void loadTable()
  }, [loadTable])

  function applyFilter(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const value = filterValue.trim()
    setCursorHistory([null])
    setAppliedFilter(value && filterColumn ? { column: filterColumn, value } : null)
  }

  async function copyCell(id: string, cell: DatabaseCell | undefined) {
    await navigator.clipboard.writeText(cellText(cell))
    setCopiedCell(id)
    window.setTimeout(() => setCopiedCell(null), 1_500)
  }

  function downloadCsv() {
    if (!page || !selected) return
    const header = page.columns.map((column) => csvValue(column.name)).join(',')
    const rows = page.rows.map((row) =>
      page.columns
        .map((column) => csvValue(cellText(row.cells[column.name])))
        .join(','),
    )
    const blob = new Blob([[header, ...rows].join('\n')], {
      type: 'text/csv;charset=utf-8',
    })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `${selected.namespace}.${selected.name}.csv`
    anchor.click()
    URL.revokeObjectURL(url)
  }

  if (!canRead) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Database access unavailable</CardTitle>
          <CardDescription>
            This administrator requires the database:read scope.
          </CardDescription>
        </CardHeader>
      </Card>
    )
  }

  return (
    <>
      <div className="grid min-h-[calc(100vh-7rem)] gap-4 lg:grid-cols-[250px_minmax(0,1fr)]">
        <Card className="min-h-0">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-base">
              <Database className="size-4 text-muted-foreground" />
              Tables
            </CardTitle>
            <CardDescription>
              {tables.length} across {new Set(tables.map((item) => item.table.namespace)).size}{' '}
              namespaces
            </CardDescription>
          </CardHeader>
          <CardContent className="min-h-0">
            <div className="relative mb-3">
              <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                value={tableSearch}
                onChange={(event) => setTableSearch(event.target.value)}
                placeholder="Find a table…"
                className="pl-9"
              />
            </div>
            {tablesQuery.error ? (
              <p className="text-sm text-destructive">{tablesQuery.error}</p>
            ) : (
              <ScrollArea className="h-[calc(100vh-15rem)]">
                <div className="grid gap-4 pr-3">
                  {namespaces.map(([namespace, summaries]) => (
                    <div key={namespace}>
                      <p className="mb-1 px-2 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
                        {namespace}
                      </p>
                      <div className="grid gap-0.5">
                        {summaries.map((summary) => (
                          <button
                            type="button"
                            key={tableKey(summary.table)}
                            onClick={() => chooseTable(summary)}
                            className={`flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-left text-sm transition-colors ${
                              selected &&
                              tableKey(selected) === tableKey(summary.table)
                                ? 'bg-primary text-primary-foreground'
                                : 'hover:bg-muted'
                            }`}
                          >
                            <span className="min-w-0 flex-1 truncate font-mono text-xs">
                              {summary.table.name}
                            </span>
                            <span className="text-[10px] tabular-nums opacity-70">
                              {displayCount(summary)}
                            </span>
                          </button>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </ScrollArea>
            )}
          </CardContent>
        </Card>

        <Card className="min-w-0">
          {!selected ? (
            <CardContent className="flex min-h-80 items-center justify-center text-sm text-muted-foreground">
              Select a table to inspect its schema and rows.
            </CardContent>
          ) : (
            <>
              <CardHeader className="flex flex-row items-start justify-between gap-4">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <span className="font-mono">
                      {selected.namespace}.{selected.name}
                    </span>
                    <Badge variant="outline">
                      {schema?.row_count
                        ? `${schema.row_count.exact ? '' : '~'}${schema.row_count.value.toLocaleString()} rows`
                        : 'row count —'}
                    </Badge>
                  </CardTitle>
                  <CardDescription>
                    Read-only inspection
                    {capabilities.data
                      ? ` · ${capabilities.data.provider}`
                      : ''}
                  </CardDescription>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={loadingRows}
                  onClick={() => void loadTable()}
                >
                  <RefreshCw
                    data-slot="icon"
                    className={loadingRows ? 'animate-spin' : undefined}
                  />
                  Refresh
                </Button>
              </CardHeader>
              <CardContent>
                {rowError && (
                  <div className="mb-4 rounded-2xl bg-destructive/10 p-3 text-sm text-destructive">
                    {rowError}
                  </div>
                )}
                <Tabs defaultValue="rows">
                  <TabsList>
                    <TabsTrigger value="rows">Rows</TabsTrigger>
                    <TabsTrigger value="schema">Schema</TabsTrigger>
                    <TabsTrigger value="relations">
                      Foreign keys
                      {schema && (
                        <Badge variant="secondary" className="ml-1">
                          {schema.foreign_keys.length}
                        </Badge>
                      )}
                    </TabsTrigger>
                  </TabsList>

                  <TabsContent value="rows" className="mt-4">
                    <div className="mb-3 flex flex-col gap-2 xl:flex-row xl:items-center">
                      <form
                        onSubmit={applyFilter}
                        className="flex min-w-0 flex-1 gap-2"
                      >
                        <Select
                          value={filterColumn || undefined}
                          onValueChange={setFilterColumn}
                        >
                          <SelectTrigger
                            aria-label="Filter column"
                            className="max-w-48 font-mono text-xs"
                          >
                            <SelectValue placeholder="Column" />
                          </SelectTrigger>
                          <SelectContent>
                            {(schema?.columns ?? []).map((column) => (
                              <SelectItem
                                key={column.name}
                                value={column.name}
                                disabled={column.redacted}
                                className="font-mono text-xs"
                              >
                                {column.name}
                                {column.primary_key ? ' (PK)' : ''}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                        <Input
                          value={filterValue}
                          onChange={(event) => setFilterValue(event.target.value)}
                          placeholder={
                            schema?.primary_key.includes(filterColumn)
                              ? 'Search by primary key…'
                              : 'Exact value…'
                          }
                          className="min-w-32 max-w-sm font-mono text-xs"
                        />
                        <Button type="submit" variant="secondary" size="sm">
                          Filter
                        </Button>
                        {appliedFilter && (
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => {
                              setFilterValue('')
                              setAppliedFilter(null)
                              setCursorHistory([null])
                            }}
                          >
                            Clear
                          </Button>
                        )}
                      </form>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-muted-foreground">
                          {page?.rows.length ?? 0} rows on this page
                        </span>
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={!page || page.rows.length === 0}
                          onClick={downloadCsv}
                        >
                          <Download data-slot="icon" /> CSV
                        </Button>
                      </div>
                    </div>

                    {page && page.rows.length > 0 ? (
                      <div className="rounded-2xl border">
                        <Table>
                          <TableHeader>
                            <TableRow>
                              {page.columns.map((column) => (
                                <TableHead key={column.name}>
                                  <span className="flex items-center gap-1.5">
                                    <span className="font-mono text-xs">
                                      {column.name}
                                    </span>
                                    {column.primary_key && (
                                      <KeyRound className="size-3 text-muted-foreground" />
                                    )}
                                  </span>
                                </TableHead>
                              ))}
                            </TableRow>
                          </TableHeader>
                          <TableBody>
                            {page.rows.map((row, rowIndex) => (
                              <TableRow key={`${cursor ?? 'first'}-${rowIndex}`}>
                                {page.columns.map((column) => {
                                  const cell = row.cells[column.name]
                                  const id = `${rowIndex}-${column.name}`
                                  return (
                                    <TableCell
                                      key={column.name}
                                      className="max-w-72"
                                    >
                                      <div className="flex items-center gap-1">
                                        <button
                                          type="button"
                                          title="Copy cell value"
                                          disabled={cell?.kind === 'redacted'}
                                          onClick={() => void copyCell(id, cell)}
                                          className={`min-w-0 flex-1 truncate text-left font-mono text-xs ${
                                            cell?.kind === 'null'
                                              ? 'italic text-muted-foreground'
                                              : cell?.kind === 'redacted'
                                                ? 'text-muted-foreground'
                                                : ''
                                          }`}
                                        >
                                          {cellText(cell)}
                                        </button>
                                        {copiedCell === id && (
                                          <Check className="size-3 text-emerald-500" />
                                        )}
                                        {cell?.kind === 'json' && cell.value && (
                                          <Button
                                            variant="ghost"
                                            size="icon-xs"
                                            title="Open JSON viewer"
                                            onClick={() =>
                                              setJsonCell({
                                                column: column.name,
                                                value: cell.value ?? '',
                                              })
                                            }
                                          >
                                            <Braces />
                                          </Button>
                                        )}
                                      </div>
                                    </TableCell>
                                  )
                                })}
                              </TableRow>
                            ))}
                          </TableBody>
                        </Table>
                      </div>
                    ) : (
                      <div className="flex h-44 items-center justify-center text-sm text-muted-foreground">
                        {loadingRows
                          ? 'Loading rows…'
                          : 'No rows match this view.'}
                      </div>
                    )}

                    <div className="mt-3 flex items-center justify-end gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={cursorHistory.length <= 1 || loadingRows}
                        onClick={() =>
                          setCursorHistory((history) => history.slice(0, -1))
                        }
                      >
                        <ChevronLeft data-slot="icon" /> Previous
                      </Button>
                      <span className="min-w-16 text-center text-xs tabular-nums text-muted-foreground">
                        Page {cursorHistory.length}
                      </span>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={!page?.next_cursor || loadingRows}
                        onClick={() =>
                          page?.next_cursor &&
                          setCursorHistory((history) => [
                            ...history,
                            page.next_cursor,
                          ])
                        }
                      >
                        Next <ChevronRight data-slot="icon" />
                      </Button>
                    </div>
                  </TabsContent>

                  <TabsContent value="schema" className="mt-4">
                    <div className="rounded-2xl border">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>Column</TableHead>
                            <TableHead>Type</TableHead>
                            <TableHead>Nullable</TableHead>
                            <TableHead>Default</TableHead>
                            <TableHead>Protection</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {(schema?.columns ?? []).map((column) => (
                            <TableRow key={column.name}>
                              <TableCell className="font-mono text-xs">
                                <span className="flex items-center gap-2">
                                  {column.name}
                                  {column.primary_key && (
                                    <Badge variant="secondary">PK</Badge>
                                  )}
                                </span>
                              </TableCell>
                              <TableCell className="font-mono text-xs text-muted-foreground">
                                {column.data_type}
                              </TableCell>
                              <TableCell>
                                {column.nullable ? 'Yes' : 'No'}
                              </TableCell>
                              <TableCell className="max-w-64 truncate font-mono text-xs text-muted-foreground">
                                {column.default ?? '—'}
                              </TableCell>
                              <TableCell>
                                {column.redacted ? (
                                  <Badge variant="outline">Redacted</Badge>
                                ) : (
                                  'Visible'
                                )}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  </TabsContent>

                  <TabsContent value="relations" className="mt-4">
                    {schema && schema.foreign_keys.length > 0 ? (
                      <div className="grid gap-3">
                        {schema.foreign_keys.map((foreignKey) => (
                          <div
                            key={foreignKey.name}
                            className="rounded-2xl border p-4"
                          >
                            <p className="font-medium">{foreignKey.name}</p>
                            <p className="mt-2 font-mono text-xs text-muted-foreground">
                              {foreignKey.columns.join(', ')} →{' '}
                              {foreignKey.referenced_table.namespace}.
                              {foreignKey.referenced_table.name} (
                              {foreignKey.referenced_columns.join(', ')})
                            </p>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="flex h-44 items-center justify-center text-sm text-muted-foreground">
                        This table has no foreign keys.
                      </div>
                    )}
                  </TabsContent>
                </Tabs>
              </CardContent>
            </>
          )}
        </Card>
      </div>

      <Sheet open={jsonCell !== null} onOpenChange={(open) => !open && setJsonCell(null)}>
        <SheetContent className="sm:max-w-xl">
          <SheetHeader>
            <SheetTitle className="flex items-center gap-2">
              <Braces className="size-4" /> {jsonCell?.column}
            </SheetTitle>
            <SheetDescription>Read-only formatted JSON value.</SheetDescription>
          </SheetHeader>
          <div className="grid min-h-0 flex-1 gap-3 px-6 pb-6">
            <Button
              variant="outline"
              size="sm"
              className="justify-self-start"
              onClick={() =>
                jsonCell &&
                void navigator.clipboard.writeText(jsonCell.value)
              }
            >
              <Copy data-slot="icon" /> Copy JSON
            </Button>
            <ScrollArea className="min-h-0 rounded-2xl border bg-muted/30">
              <pre className="p-4 font-mono text-xs leading-relaxed whitespace-pre-wrap">
                {jsonCell ? prettyJson(jsonCell.value) : ''}
              </pre>
            </ScrollArea>
          </div>
        </SheetContent>
      </Sheet>
    </>
  )
}
