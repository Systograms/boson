import { SiteFooter } from "@/components/site-footer"
import { SiteHeader } from "@/components/site-header"
import { getAllDocs } from "@/lib/docs"

export default function DocsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const docs = getAllDocs()

  return (
    <>
      <SiteHeader docs={docs} />
      <div className="mx-auto flex w-full max-w-6xl flex-1 gap-12 px-4 py-12 sm:px-6">
        {children}
      </div>
      <SiteFooter />
    </>
  )
}
