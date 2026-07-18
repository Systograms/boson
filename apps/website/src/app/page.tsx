import Link from "next/link"
import {
  ArrowRight,
  Blocks,
  Boxes,
  FileStack,
  Fingerprint,
  GitBranch,
  Layers,
  ScrollText,
  Send,
  TerminalSquare,
  Users,
} from "lucide-react"
import { GitHubIcon } from "@/components/icons"
import { SiteFooter } from "@/components/site-footer"
import { SiteHeader } from "@/components/site-header"
import { TerminalCard } from "@/components/terminal-card"
import { Button } from "@/components/ui/button"
import { getAllDocs } from "@/lib/docs"

const CAPABILITIES = [
  {
    icon: Fingerprint,
    title: "Identity",
    body: "End-user accounts with Argon2id passwords, JWT sessions, and refresh tokens out of the box.",
  },
  {
    icon: Users,
    title: "Organizations",
    body: "Multi-tenant orgs with role memberships, invitations, and authorization built in.",
  },
  {
    icon: FileStack,
    title: "Files",
    body: "Uploads and downloads behind a provider-agnostic ObjectStore port. Local disk today, S3 tomorrow.",
  },
  {
    icon: Boxes,
    title: "Jobs & queue",
    body: "Durable at-least-once background jobs on PostgreSQL, with Admin inspection and manual retry.",
  },
  {
    icon: Send,
    title: "Notifications",
    body: "Event-driven email rendering with durable delivery history and an idempotent dev mailbox.",
  },
  {
    icon: ScrollText,
    title: "Events & audit",
    body: "Versioned events through a transactional outbox, plus an immutable, redacted audit trail.",
  },
]

const PRINCIPLES = [
  {
    icon: Layers,
    title: "Capabilities own their data",
    body: "Each capability owns its schema and talks to the rest of the platform through versioned contracts — never through another capability's tables.",
  },
  {
    icon: GitBranch,
    title: "Ports over vendors",
    body: "Storage, queue, and mailer are small ports. Provider SDKs live in adapters at the edge, so swapping vendors never touches product logic.",
  },
  {
    icon: TerminalSquare,
    title: "Operations included",
    body: "The Admin API, dashboard, and CLI ship with the platform: health, request traces, jobs, events, and audit without bolting on separate tooling.",
  },
]

export default function HomePage() {
  const docs = getAllDocs()

  return (
    <>
      <SiteHeader docs={docs} />
      <main className="flex-1">
        {/* Hero */}
        <section className="relative overflow-hidden">
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(ellipse_60%_60%_at_50%_-10%,var(--muted),transparent)]"
          />
          <div
            aria-hidden
            className="pointer-events-none absolute inset-0 -z-10 bg-[linear-gradient(to_right,var(--border)_1px,transparent_1px),linear-gradient(to_bottom,var(--border)_1px,transparent_1px)] bg-[size:72px_72px] opacity-[0.35] [mask-image:radial-gradient(ellipse_70%_60%_at_50%_0%,black,transparent)]"
          />
          <div className="mx-auto grid max-w-6xl items-center gap-12 px-4 pt-20 pb-20 sm:px-6 sm:pt-28 sm:pb-24 lg:grid-cols-[1.1fr_1fr] lg:gap-16">
            <div>
              <Link
                href="/docs/architecture"
                className="inline-flex items-center gap-2 rounded-full border border-border bg-card px-3 py-1 text-xs font-medium text-muted-foreground shadow-2xs transition-colors hover:text-foreground"
              >
                <span className="inline-block size-1.5 rounded-full bg-chart-2" />
                Server, worker, dashboard & CLI — one platform
                <ArrowRight className="size-3" />
              </Link>
              <h1 className="mt-6 text-5xl leading-[1.05] font-semibold tracking-tight text-balance sm:text-6xl">
                The backend platform that stays out of your way
              </h1>
              <p className="mt-6 max-w-lg text-lg leading-relaxed text-muted-foreground text-pretty">
                Boson is a modular backend written in Rust. Identity, files,
                jobs, and events ship as composable capabilities — with
                operations built in and no coupling to any cloud provider.
              </p>
              <div className="mt-8 flex flex-wrap items-center gap-3">
                <Button size="lg" asChild>
                  <Link href="/docs">
                    Get started
                    <ArrowRight className="size-4" />
                  </Link>
                </Button>
                <Button size="lg" variant="outline" asChild>
                  <a
                    href="https://github.com/Systograms/boson"
                    target="_blank"
                    rel="noreferrer"
                  >
                    <GitHubIcon className="size-4" />
                    Star on GitHub
                  </a>
                </Button>
              </div>
              <p className="mt-6 font-mono text-xs text-muted-foreground">
                Rust · PostgreSQL · Provider-agnostic ports
              </p>
            </div>
            <TerminalCard
              title="boson — zsh"
              className="w-full max-w-lg justify-self-center lg:justify-self-end"
              lines={[
                { kind: "command", text: "boson start" },
                { kind: "output", text: "[postgres] ready" },
                { kind: "output", text: "[migrate] all migrations applied" },
                { kind: "output", text: "[server] http://localhost:8080" },
                { kind: "output", text: "[worker] started" },
                { kind: "output", text: "[dashboard] http://localhost:3000" },
                { kind: "blank" },
                { kind: "output", text: "[boson] running · press Ctrl+C to stop" },
              ]}
            />
          </div>
        </section>

        {/* Capabilities */}
        <section className="border-t border-border bg-muted/30">
          <div className="mx-auto max-w-6xl px-4 py-20 sm:px-6 sm:py-24">
            <div className="max-w-2xl">
              <p className="text-sm font-medium text-muted-foreground">
                Batteries included
              </p>
              <h2 className="mt-3 text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
                Everything a product backend needs, as capabilities
              </h2>
              <p className="mt-4 text-base leading-relaxed text-muted-foreground">
                Skip the boilerplate. Each capability is production-grade,
                owns its own schema, and composes with the rest through
                stable contracts.
              </p>
            </div>
            <div className="mt-12 grid gap-px overflow-hidden rounded-xl border border-border bg-border sm:grid-cols-2 lg:grid-cols-3">
              {CAPABILITIES.map((item) => (
                <div
                  key={item.title}
                  className="group bg-card p-6 transition-colors hover:bg-accent/40"
                >
                  <div className="flex size-9 items-center justify-center rounded-lg border border-border bg-background shadow-2xs">
                    <item.icon className="size-4.5 text-foreground" />
                  </div>
                  <h3 className="mt-4 text-sm font-semibold tracking-tight">
                    {item.title}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                    {item.body}
                  </p>
                </div>
              ))}
            </div>
          </div>
        </section>

        {/* Architecture */}
        <section className="border-t border-border">
          <div className="mx-auto grid max-w-6xl items-center gap-12 px-4 py-20 sm:px-6 sm:py-24 lg:grid-cols-2 lg:gap-16">
            <div>
              <p className="text-sm font-medium text-muted-foreground">
                Architecture
              </p>
              <h2 className="mt-3 text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
                One dependency direction. No leaks.
              </h2>
              <div className="mt-8 space-y-8">
                {PRINCIPLES.map((item) => (
                  <div key={item.title} className="flex gap-4">
                    <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-card shadow-2xs">
                      <item.icon className="size-4.5" />
                    </div>
                    <div>
                      <h3 className="text-sm font-semibold tracking-tight">
                        {item.title}
                      </h3>
                      <p className="mt-1.5 text-sm leading-relaxed text-muted-foreground">
                        {item.body}
                      </p>
                    </div>
                  </div>
                ))}
              </div>
              <Button variant="outline" className="mt-8" asChild>
                <Link href="/docs/architecture">
                  Explore the architecture
                  <ArrowRight className="size-4" />
                </Link>
              </Button>
            </div>
            <div className="rounded-xl border border-border bg-muted/40 p-6 sm:p-8">
              <div className="space-y-3">
                <DiagramRow label="Clients" items={["Dashboard", "CLI"]} />
                <DiagramArrow label="Admin API" />
                <DiagramRow label="Apps" items={["Server", "Worker"]} />
                <DiagramArrow label="contracts" />
                <DiagramRow
                  label="Capabilities"
                  items={["Identity", "Orgs", "Files", "Jobs", "Events"]}
                />
                <DiagramArrow label="ports" />
                <DiagramRow
                  label="Adapters"
                  items={["PostgreSQL", "Storage", "Mailer"]}
                />
              </div>
              <p className="mt-6 text-center font-mono text-xs text-muted-foreground">
                Provider SDKs never appear above the ports line
              </p>
            </div>
          </div>
        </section>

        {/* Quickstart */}
        <section className="border-t border-border bg-muted/30">
          <div className="mx-auto grid max-w-6xl items-center gap-12 px-4 py-20 sm:px-6 sm:py-24 lg:grid-cols-2 lg:gap-16">
            <TerminalCard
              title="quickstart"
              className="order-2 w-full max-w-lg justify-self-center lg:order-1 lg:justify-self-start"
              lines={[
                { kind: "command", text: "boson init my-app && cd my-app" },
                { kind: "output", text: "created Boson app `my-app`" },
                { kind: "blank" },
                { kind: "command", text: "boson start" },
                { kind: "output", text: "[postgres] ready" },
                { kind: "output", text: "[migrate] all migrations applied" },
                { kind: "output", text: "[server] http://localhost:8080" },
                { kind: "output", text: "[dashboard] http://localhost:3000" },
              ]}
            />
            <div className="order-1 lg:order-2">
              <p className="text-sm font-medium text-muted-foreground">
                Quickstart
              </p>
              <h2 className="mt-3 text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
                From clone to running platform in one command
              </h2>
              <p className="mt-4 text-base leading-relaxed text-muted-foreground">
                The Boson CLI validates your machine, starts managed
                infrastructure, applies migrations, launches every process,
                waits for health, and streams one unified log.
              </p>
              <div className="mt-8 flex flex-wrap gap-3">
                <Button asChild>
                  <Link href="/docs/getting-started">
                    Read the setup guide
                    <ArrowRight className="size-4" />
                  </Link>
                </Button>
              </div>
            </div>
          </div>
        </section>

        {/* CTA */}
        <section className="border-t border-border">
          <div className="mx-auto flex max-w-6xl flex-col items-center px-4 py-20 text-center sm:px-6 sm:py-24">
            <div className="flex size-12 items-center justify-center rounded-xl border border-border bg-card shadow-sm">
              <Blocks className="size-6" />
            </div>
            <h2 className="mt-6 max-w-xl text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
              Own your backend. Keep your options.
            </h2>
            <p className="mt-4 max-w-md text-base leading-relaxed text-muted-foreground">
              Boson is open source and runs anywhere PostgreSQL does — your
              laptop, your VPC, or any cloud.
            </p>
            <div className="mt-8 flex flex-wrap items-center justify-center gap-3">
              <Button size="lg" asChild>
                <Link href="/docs">
                  Read the docs
                  <ArrowRight className="size-4" />
                </Link>
              </Button>
              <Button size="lg" variant="outline" asChild>
                <a
                  href="https://github.com/Systograms/boson"
                  target="_blank"
                  rel="noreferrer"
                >
                  <GitHubIcon className="size-4" />
                  GitHub
                </a>
              </Button>
            </div>
          </div>
        </section>
      </main>
      <SiteFooter />
    </>
  )
}

function DiagramRow({ label, items }: { label: string; items: string[] }) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-24 shrink-0 text-right font-mono text-[11px] text-muted-foreground">
        {label}
      </span>
      <div className="flex min-w-0 flex-1 flex-wrap gap-2">
        {items.map((item) => (
          <span
            key={item}
            className="rounded-md border border-border bg-card px-2.5 py-1.5 text-xs font-medium shadow-2xs"
          >
            {item}
          </span>
        ))}
      </div>
    </div>
  )
}

function DiagramArrow({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-24 shrink-0" />
      <div className="flex items-center gap-2 pl-4">
        <span className="h-5 w-px bg-border" />
        <span className="font-mono text-[10px] tracking-wide text-muted-foreground uppercase">
          {label}
        </span>
      </div>
    </div>
  )
}
