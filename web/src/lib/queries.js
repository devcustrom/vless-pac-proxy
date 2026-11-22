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
    // Периодический refetch делает App.vue: @pinia/colada 1.4.9
    // не поддерживает refetchInterval.
    query: () => getJson('/api/status'),
  })
}

export function useWgConfigsQuery() {
  return useQuery({
    key: ['wg-configs'],
    query: () => getJson('/api/wg/configs'),
  })
}

export function useXrayConfigsQuery() {
  return useQuery({
    key: ['xray-configs'],
    query: () => getJson('/api/xray/configs'),
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
    onSuccess: async () => {
      // Ждём refetch: isLoading не снимется, пока переключатель не получит
      // новое состояние. Ошибка refetch не должна превращать успешный POST
      // в ошибку — offline-баннер покажет потерю связи.
      await Promise.all(
        keys.map((key) =>
          cache.invalidateQueries({ key }).catch(() => {
            /* статус обновится polling'ом */
          }),
        ),
      )
    },
    onError: (error) => notify(error.message, 'error'),
  })
}
