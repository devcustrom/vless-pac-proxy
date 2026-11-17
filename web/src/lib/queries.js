import { useMutation, useQuery, useQueryCache } from '@pinia/colada'
import { notify } from './store'

async function getJson(path) {
  const response = await fetch(path)
  const data = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`)
  return data
}

export function useStatusQuery() {
  return useQuery({
    key: ['status'],
    query: () => getJson('/api/status'),
    refetchInterval: 5000,
  })
}

export function useWgConfigsQuery() {
  return useQuery({
    key: ['wg-configs'],
    query: () => getJson('/api/wg/configs'),
  })
}

/**
 * Обёртка над useMutation: тост об ошибке и инвалидация запросов.
 * keys — например [['status'], ['wg-configs']].
 */
export function useActionMutation(action, keys = [['status']]) {
  const cache = useQueryCache()
  return useMutation({
    mutation: action,
    onSuccess: () => {
      for (const key of keys) {
        cache.invalidateQueries({ key })
      }
    },
    onError: (error) => notify(error.message, 'error'),
  })
}
