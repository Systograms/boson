import Link from "next/link"
import type { DocMeta } from "@/lib/docs"
import { cn } from "@/lib/utils"

type DocsSidebarProps = {
  docs: DocMeta[]
  activeSlug?: string
}

export function DocsSidebar({ docs, activeSlug }: DocsSidebarProps) {
  return (
    <aside className="hidden w-60 shrink-0 md:block">
      <div className="sticky top-24">
        <p className="mb-3 px-2.5 text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Documentation
        </p>
        <nav className="relative flex flex-col gap-0.5">
          {docs.map((doc) => {
            const active = doc.slug === activeSlug
            return (
              <Link
                key={doc.slug}
                href={`/docs/${doc.slug}`}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "relative rounded-md px-2.5 py-1.5 text-sm transition-colors",
                  active
                    ? "bg-accent font-medium text-accent-foreground"
                    : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
                )}
              >
                {doc.title}
              </Link>
            )
          })}
        </nav>
      </div>
    </aside>
  )
}
