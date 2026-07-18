export type ScopeSource =
  | readonly string[]
  | { readonly scopes: readonly string[]; readonly bootstrap?: boolean }
  | null
  | undefined

export function hasScope(source: ScopeSource, required: string): boolean {
  if (!source) return false
  if ('scopes' in source) {
    return (
      source.bootstrap === true ||
      source.scopes.includes('*') ||
      source.scopes.includes(required)
    )
  }
  return source.includes('*') || source.includes(required)
}
