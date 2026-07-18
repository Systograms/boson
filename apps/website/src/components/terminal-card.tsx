import { cn } from "@/lib/utils"

type TerminalLine = {
  kind: "command" | "output" | "blank"
  text?: string
}

type TerminalCardProps = {
  title: string
  lines: TerminalLine[]
  className?: string
}

export function TerminalCard({ title, lines, className }: TerminalCardProps) {
  return (
    <div
      className={cn(
        "overflow-hidden rounded-xl border border-border bg-card shadow-md",
        className,
      )}
    >
      <div className="flex items-center gap-2 border-b border-border bg-muted/60 px-4 py-2.5">
        <span className="size-2.5 rounded-full bg-border" />
        <span className="size-2.5 rounded-full bg-border" />
        <span className="size-2.5 rounded-full bg-border" />
        <span className="ml-2 font-mono text-xs text-muted-foreground">
          {title}
        </span>
      </div>
      <div className="overflow-x-auto p-4 font-mono text-[13px] leading-6">
        {lines.map((line, i) => {
          if (line.kind === "blank") {
            return <div key={i} className="h-3" />
          }
          if (line.kind === "command") {
            return (
              <div key={i} className="whitespace-pre text-foreground">
                <span className="select-none text-muted-foreground">$ </span>
                {line.text}
              </div>
            )
          }
          return (
            <div key={i} className="whitespace-pre text-muted-foreground">
              {line.text}
            </div>
          )
        })}
      </div>
    </div>
  )
}
