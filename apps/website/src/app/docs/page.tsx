import { redirect } from "next/navigation"
import { getAllDocs } from "@/lib/docs"

export default function DocsIndexPage() {
  const docs = getAllDocs()
  const first = docs[0]
  redirect(first ? `/docs/${first.slug}` : "/")
}
