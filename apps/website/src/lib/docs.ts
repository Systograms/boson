import fs from "node:fs"
import path from "node:path"
import matter from "gray-matter"

const DOCS_DIR = path.join(process.cwd(), "content/docs")

export type DocMeta = {
  slug: string
  title: string
  description: string
  order: number
}

export type Doc = DocMeta & {
  content: string
}

function readDocFile(slug: string): Doc | null {
  const filePath = path.join(DOCS_DIR, `${slug}.mdx`)
  if (!fs.existsSync(filePath)) {
    return null
  }

  const raw = fs.readFileSync(filePath, "utf8")
  const { data, content } = matter(raw)

  return {
    slug,
    title: String(data.title ?? slug),
    description: String(data.description ?? ""),
    order: Number(data.order ?? 99),
    content,
  }
}

export function getAllDocs(): DocMeta[] {
  if (!fs.existsSync(DOCS_DIR)) {
    return []
  }

  return fs
    .readdirSync(DOCS_DIR)
    .filter((name) => name.endsWith(".mdx"))
    .map((name) => {
      const slug = name.replace(/\.mdx$/, "")
      const doc = readDocFile(slug)
      if (!doc) {
        return null
      }
      return {
        slug: doc.slug,
        title: doc.title,
        description: doc.description,
        order: doc.order,
      }
    })
    .filter((doc): doc is DocMeta => doc !== null)
    .sort((a, b) => a.order - b.order)
}

export function getDoc(slug: string): Doc | null {
  return readDocFile(slug)
}

export function getDocSlugs(): string[] {
  return getAllDocs().map((doc) => doc.slug)
}
