import * as React from 'react'
import { ResponsiveContainer, Tooltip } from 'recharts'
import { cn } from '@/lib/utils'

export type ChartConfig = Record<string, { label: string; color?: string }>

const ChartContext = React.createContext<ChartConfig | null>(null)

function useChartConfig(): ChartConfig {
  const config = React.useContext(ChartContext)
  if (!config) {
    throw new Error('Chart components must be used within a ChartContainer')
  }
  return config
}

export function ChartContainer({
  config,
  className,
  children,
}: {
  config: ChartConfig
  className?: string
  children: React.ReactElement
}) {
  // Expose each series color as --color-<key> so charts can reference them
  // the same way shadcn charts do.
  const style = Object.fromEntries(
    Object.entries(config)
      .filter(([, entry]) => entry.color)
      .map(([key, entry]) => [`--color-${key}`, entry.color]),
  ) as React.CSSProperties

  return (
    <ChartContext.Provider value={config}>
      <div
        data-slot="chart"
        style={style}
        className={cn(
          "h-56 w-full text-xs [&_.recharts-cartesian-axis-tick_text]:fill-muted-foreground [&_.recharts-cartesian-grid_line[stroke='#ccc']]:stroke-border/60 [&_.recharts-curve.recharts-tooltip-cursor]:stroke-border [&_.recharts-rectangle.recharts-tooltip-cursor]:fill-muted/50 [&_.recharts-sector]:outline-hidden [&_.recharts-sector[stroke='#fff']]:stroke-transparent [&_.recharts-surface]:outline-hidden",
          className,
        )}
      >
        <ResponsiveContainer width="100%" height="100%">
          {children}
        </ResponsiveContainer>
      </div>
    </ChartContext.Provider>
  )
}

export const ChartTooltip = Tooltip

type TooltipItem = {
  dataKey?: string | number
  name?: string | number
  value?: number | string
  color?: string
}

export function ChartTooltipContent({
  active,
  payload,
  label,
  valueFormatter,
}: {
  active?: boolean
  payload?: TooltipItem[]
  label?: unknown
  valueFormatter?: (value: number) => string
}) {
  const config = useChartConfig()
  if (!active || !payload || payload.length === 0) return null

  return (
    <div className="min-w-36 rounded-lg border bg-popover px-3 py-2 text-xs shadow-md">
      {label != null && label !== '' && (
        <p className="mb-1.5 font-medium text-popover-foreground">
          {String(label)}
        </p>
      )}
      <div className="grid gap-1">
        {payload.map((item, index) => {
          const key = String(item.dataKey ?? item.name ?? index)
          const entry = config[key]
          const value =
            typeof item.value === 'number' && valueFormatter
              ? valueFormatter(item.value)
              : item.value
          return (
            <div key={key} className="flex items-center gap-2">
              <span
                className="size-2 shrink-0 rounded-full"
                style={{ background: entry?.color ?? item.color }}
              />
              <span className="text-muted-foreground">
                {entry?.label ?? key}
              </span>
              <span className="ml-auto pl-4 font-medium tabular-nums text-popover-foreground">
                {value ?? '—'}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
