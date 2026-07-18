import Link from "next/link"
import { Menu } from "lucide-react"
import { GitHubIcon } from "@/components/icons"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet"
import type { DocMeta } from "@/lib/docs"
import { cn } from "@/lib/utils"

type SiteHeaderProps = {
  docs?: DocMeta[]
  className?: string
}

export function SiteHeader({ docs = [], className }: SiteHeaderProps) {
  return (
    <header
      className={cn(
        "sticky top-0 z-40 border-b border-border/60 bg-background/80 backdrop-blur-md",
        className,
      )}
    >
      <div className="mx-auto flex h-14 max-w-6xl items-center gap-6 px-4 sm:px-6">
        <Link
          href="/"
          className="flex items-center gap-2 font-semibold tracking-tight"
        >
          <span className="flex size-6 items-center justify-center rounded-md bg-primary font-mono text-xs font-bold text-primary-foreground">
            b
          </span>
          Boson
        </Link>
        <nav className="hidden items-center gap-5 text-sm sm:flex">
          <Link
            href="/docs"
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            Documentation
          </Link>
          <Link
            href="/docs/architecture"
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            Architecture
          </Link>
        </nav>
        <div className="ml-auto flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="icon"
            className="hidden text-muted-foreground hover:text-foreground sm:inline-flex"
            asChild
          >
            <a
              href="https://github.com/Systograms/boson"
              target="_blank"
              rel="noreferrer"
              aria-label="Boson on GitHub"
            >
              <GitHubIcon className="size-4" />
            </a>
          </Button>
          <ThemeToggle />
          {docs.length > 0 ? (
            <Sheet>
              <SheetTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="sm:hidden"
                  aria-label="Open navigation menu"
                >
                  <Menu className="size-4" />
                </Button>
              </SheetTrigger>
              <SheetContent side="left" className="w-72">
                <SheetHeader>
                  <SheetTitle>Documentation</SheetTitle>
                </SheetHeader>
                <nav className="mt-2 flex flex-col gap-0.5 px-3">
                  {docs.map((doc) => (
                    <Link
                      key={doc.slug}
                      href={`/docs/${doc.slug}`}
                      className="rounded-md px-2.5 py-2 text-sm text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
                    >
                      {doc.title}
                    </Link>
                  ))}
                </nav>
              </SheetContent>
            </Sheet>
          ) : null}
        </div>
      </div>
    </header>
  )
}
