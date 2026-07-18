import type { Metadata } from "next"
import { notFound } from "next/navigation"
import { DocsPager } from "@/components/docs-pager"
import { DocsSidebar } from "@/components/docs-sidebar"
import { MdxContent } from "@/components/mdx-content"
import { getAllDocs, getDoc, getDocSlugs } from "@/lib/docs"

type DocsPageProps = {
  params: Promise<{ slug: string }>
}

export function generateStaticParams() {
  return getDocSlugs().map((slug) => ({ slug }))
}

export async function generateMetadata({
  params,
}: DocsPageProps): Promise<Metadata> {
  const { slug } = await params
  const doc = getDoc(slug)
  if (!doc) {
    return { title: "Docs" }
  }
  return {
    title: doc.title,
    description: doc.description,
  }
}

export default async function DocsPage({ params }: DocsPageProps) {
  const { slug } = await params
  const doc = getDoc(slug)
  if (!doc) {
    notFound()
  }

  const docs = getAllDocs()
  const index = docs.findIndex((d) => d.slug === doc.slug)
  const prev = index > 0 ? docs[index - 1] : null
  const next = index >= 0 && index < docs.length - 1 ? docs[index + 1] : null

  return (
    <>
      <DocsSidebar docs={docs} activeSlug={doc.slug} />
      <article className="min-w-0 max-w-3xl flex-1 pb-16">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span>Docs</span>
          <span className="text-border">/</span>
          <span className="text-foreground">{doc.title}</span>
        </div>
        <h1 className="mt-4 text-4xl font-semibold tracking-tight text-balance">
          {doc.title}
        </h1>
        {doc.description ? (
          <p className="mt-3 text-lg leading-relaxed text-muted-foreground">
            {doc.description}
          </p>
        ) : null}
        <div className="mt-10">
          <MdxContent source={doc.content} />
        </div>
        <DocsPager prev={prev} next={next} />
      </article>
    </>
  )
}
