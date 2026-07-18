import Link from "next/link"
import { ArrowLeft, ArrowRight } from "lucide-react"
import type { DocMeta } from "@/lib/docs"

type DocsPagerProps = {
  prev: DocMeta | null
  next: DocMeta | null
}

export function DocsPager({ prev, next }: DocsPagerProps) {
  if (!prev && !next) {
    return null
  }

  return (
    <nav className="mt-14 grid gap-3 border-t border-border pt-8 sm:grid-cols-2">
      {prev ? (
        <Link
          href={`/docs/${prev.slug}`}
          className="group flex flex-col rounded-xl border border-border bg-card p-4 shadow-2xs transition-colors hover:bg-accent/40"
        >
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            <ArrowLeft className="size-3 transition-transform group-hover:-translate-x-0.5" />
            Previous
          </span>
          <span className="mt-1.5 text-sm font-semibold tracking-tight">
            {prev.title}
          </span>
        </Link>
      ) : (
        <span className="hidden sm:block" />
      )}
      {next ? (
        <Link
          href={`/docs/${next.slug}`}
          className="group flex flex-col items-end rounded-xl border border-border bg-card p-4 text-right shadow-2xs transition-colors hover:bg-accent/40"
        >
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            Next
            <ArrowRight className="size-3 transition-transform group-hover:translate-x-0.5" />
          </span>
          <span className="mt-1.5 text-sm font-semibold tracking-tight">
            {next.title}
          </span>
        </Link>
      ) : null}
    </nav>
  )
}
