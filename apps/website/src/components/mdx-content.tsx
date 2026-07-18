import { MDXRemote } from "next-mdx-remote/rsc"
import remarkGfm from "remark-gfm"

type MdxContentProps = {
  source: string
}

export function MdxContent({ source }: MdxContentProps) {
  return (
    <div className="prose-docs">
      <MDXRemote
        source={source}
        options={{
          mdxOptions: {
            remarkPlugins: [remarkGfm],
          },
        }}
      />
    </div>
  )
}
