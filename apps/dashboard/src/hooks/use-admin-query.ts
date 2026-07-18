import { useCallback, useEffect, useState } from 'react'
import { AdminApiError, adminGet } from '@/lib/api'

type QueryState<T> = {
  data: T | null
  error: string | null
  unauthorized: boolean
  loading: boolean
}

export function useAdminQuery<T>(endpoint: string, refreshMs?: number) {
  const [state, setState] = useState<QueryState<T>>({
    data: null,
    error: null,
    unauthorized: false,
    loading: true,
  })

  const load = useCallback(async () => {
    try {
      const data = await adminGet<T>(endpoint)
      setState({ data, error: null, unauthorized: false, loading: false })
    } catch (reason) {
      const unauthorized =
        reason instanceof AdminApiError && reason.status === 401
      setState((previous) => ({
        data: previous.data,
        error: reason instanceof Error ? reason.message : 'Request failed',
        unauthorized,
        loading: false,
      }))
    }
  }, [endpoint])

  useEffect(() => {
    setState((previous) => ({ ...previous, loading: true }))
    void load()
    if (!refreshMs) return
    const interval = setInterval(() => void load(), refreshMs)
    return () => clearInterval(interval)
  }, [load, refreshMs])

  return { ...state, refresh: load }
}
