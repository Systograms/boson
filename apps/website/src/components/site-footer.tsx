import Link from "next/link"
import { GitHubIcon } from "@/components/icons"

const FOOTER_LINKS: { title: string; links: { label: string; href: string; external?: boolean }[] }[] = [
  {
    title: "Start",
    links: [
      { label: "Introduction", href: "/docs/introduction" },
      { label: "Getting started", href: "/docs/getting-started" },
      { label: "Project structure", href: "/docs/project-structure" },
      { label: "CLI reference", href: "/docs/cli-reference" },
    ],
  },
  {
    title: "Build",
    links: [
      { label: "Local lifecycle", href: "/docs/local-lifecycle" },
      { label: "Configuration", href: "/docs/configuration" },
      { label: "Capability SDK", href: "/docs/extensions" },
      { label: "JavaScript SDK", href: "/docs/js-sdk" },
      { label: "Example project", href: "/docs/example-project" },
    ],
  },
  {
    title: "Platform",
    links: [
      { label: "Architecture", href: "/docs/architecture" },
      { label: "Admin API", href: "/docs/admin-api" },
      { label: "Deployment", href: "/docs/deployment" },
      {
        label: "Source code",
        href: "https://github.com/Systograms/boson",
        external: true,
      },
    ],
  },
]

export function SiteFooter() {
  return (
    <footer className="border-t border-border bg-muted/30">
      <div className="mx-auto max-w-6xl px-4 py-12 sm:px-6">
        <div className="flex flex-col justify-between gap-10 sm:flex-row">
          <div className="max-w-xs">
            <Link href="/" className="text-base font-semibold tracking-tight">
              Boson
            </Link>
            <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
              A modular backend platform in Rust. Capabilities, ports, and
              operations — without vendor lock-in.
            </p>
            <a
              href="https://github.com/Systograms/boson"
              target="_blank"
              rel="noreferrer"
              aria-label="Boson on GitHub"
              className="mt-4 inline-flex size-8 items-center justify-center rounded-md border border-border bg-card text-muted-foreground shadow-2xs transition-colors hover:text-foreground"
            >
              <GitHubIcon className="size-4" />
            </a>
          </div>
          <div className="grid grid-cols-2 gap-10 sm:grid-cols-3 sm:gap-12">
            {FOOTER_LINKS.map((group) => (
              <div key={group.title}>
                <h3 className="text-sm font-semibold tracking-tight">
                  {group.title}
                </h3>
                <ul className="mt-3 space-y-2.5">
                  {group.links.map((link) =>
                    link.external ? (
                      <li key={link.label}>
                        <a
                          href={link.href}
                          target="_blank"
                          rel="noreferrer"
                          className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                        >
                          {link.label}
                        </a>
                      </li>
                    ) : (
                      <li key={link.label}>
                        <Link
                          href={link.href}
                          className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                        >
                          {link.label}
                        </Link>
                      </li>
                    ),
                  )}
                </ul>
              </div>
            ))}
          </div>
        </div>
        <div className="mt-12 border-t border-border pt-6">
          <p className="text-xs text-muted-foreground">
            Open source. Runs anywhere PostgreSQL does.
          </p>
        </div>
      </div>
    </footer>
  )
}
