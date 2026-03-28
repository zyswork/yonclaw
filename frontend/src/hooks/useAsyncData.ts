/**
 * 通用异步数据加载 Hook — 替代各页面中重复的 useState+useEffect+loading+error 模式
 */
import { useState, useEffect, useCallback } from 'react'

interface UseAsyncDataResult<T> {
  data: T
  loading: boolean
  error: string
  refetch: () => Promise<void>
}

export function useAsyncData<T>(
  fetchFn: () => Promise<T>,
  initialData: T,
  deps: any[] = [],
): UseAsyncDataResult<T> {
  const [data, setData] = useState<T>(initialData)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const refetch = useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const result = await fetchFn()
      setData(result)
    } catch (e) {
      setError(String(e))
      console.error('useAsyncData fetch failed:', e)
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)

  useEffect(() => { refetch() }, [refetch])

  return { data, loading, error, refetch }
}
