<script setup>
import { computed, ref } from 'vue'
import { Activity, ListFilter, Pin, Plus, Trash2 } from 'lucide-vue-next'
import { api, notify, post } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Dialog from '@/components/ui/Dialog.vue'
import DomainAddForm from '@/components/DomainAddForm.vue'

const props = defineProps({ s: { type: Object, required: true } })

const addAction = useActionMutation(
  ({ domain, direct }) =>
    post('/api/extra', { action: direct ? 'direct-add' : 'add', domain }),
  [['status']],
)
const removeAction = useActionMutation(
  ({ domain, direct }) =>
    post('/api/extra', { action: direct ? 'direct-remove' : 'remove', domain }),
  [['status']],
)
const checkAction = useActionMutation(
  (domain) => api('/api/routes/check', domain ? { domain } : {}),
  [['status']],
)

const addOpen = ref(false)
const addDirect = ref(false)
const checking = ref('')
const removing = ref('')
const pinning = ref('')

const autoDomains = computed(() => props.s.extra || [])
const directDomains = computed(() => props.s.directDomains || [])
const routes = computed(() => props.s.autoRoutes || {})
const vpnOnly = computed(() => new Set(props.s.vpnOnly || []))

function isVpnOnly(domain) {
  return vpnOnly.value.has(domain)
}

function routeInfo(domain) {
  return routes.value[domain] || null
}

function pinnedChannel(domain) {
  const info = routeInfo(domain)
  const items = []
  if (info?.wireguard && info.wireguard.code !== '000') {
    items.push({ name: 'WireGuard', ms: info.wireguard.ms })
  }
  if (info?.vless && info.vless.code !== '000') {
    items.push({ name: 'VLESS', ms: info.vless.ms })
  }
  if (!items.length) return props.s.vpnDefault === 'vless' ? 'VLESS' : 'WireGuard'
  return items.sort((a, b) => a.ms - b.ms)[0].name
}

function routeBadge(domain) {
  if (isVpnOnly(domain)) {
    return { variant: 'success', text: `Всегда VPN · ${pinnedChannel(domain)}` }
  }
  const info = routeInfo(domain)
  if (!info) return { variant: 'outline', text: 'не проверено' }
  if (info.available === false) return { variant: 'destructive', text: 'нет ответа' }
  const ms = info.latencyMs ? ` · ${info.latencyMs} мс` : ''
  if (info.route === 'wireguard') return { variant: 'success', text: `WireGuard${ms}` }
  if (info.route === 'vless') return { variant: 'warning', text: `VLESS${ms}` }
  if (info.viaZapret) return { variant: 'secondary', text: `Напрямую · Zapret${ms}` }
  return { variant: 'outline', text: `Напрямую${ms}` }
}

function routeTitle(domain) {
  if (isVpnOnly(domain)) {
    return 'Всегда через VPN: прямой ответ 200 может быть региональной заглушкой'
  }
  const info = routeInfo(domain)
  if (!info) return 'Домен ещё не проверялся'
  const part = (label, item) =>
    item ? `${label}: ${item.code === '000' ? 'нет ответа' : item.code}, ${item.ms} мс` : null
  return [
    part('напрямую', info.direct),
    part('WireGuard', info.wireguard),
    part('VLESS', info.vless),
    info.checkedAt ? `проверено ${new Date(info.checkedAt * 1000).toLocaleString()}` : null,
  ]
    .filter(Boolean)
    .join('\n')
}

function openAdd(direct = false) {
  addDirect.value = direct
  addOpen.value = true
}

async function add(domain) {
  try {
    await addAction.mutateAsync({ domain, direct: addDirect.value })
    notify('Домен добавлен: ' + domain)
    return true
  } catch {
    return false
  }
}

async function submitAdd(output) {
  const ok = await add(output.domain)
  if (ok) addOpen.value = false
  return ok
}

async function remove(domain, direct) {
  if (!confirm(`Убрать ${domain} из «${direct ? 'Напрямую' : 'авто-списка'}»?`)) return
  const key = `${direct ? 'direct' : 'auto'}:${domain}`
  removing.value = key
  try {
    await removeAction.mutateAsync({ domain, direct })
    notify('Домен убран')
  } catch {
    /* тост показан */
  } finally {
    removing.value = ''
  }
}

async function toggleVpnOnly(domain) {
  const pinned = isVpnOnly(domain)
  pinning.value = domain
  try {
    await post('/api/extra', {
      action: pinned ? 'vpn-only-remove' : 'vpn-only-add',
      domain,
    })
    notify(pinned ? `${domain}: снова авто` : `${domain}: всегда через VPN`)
  } catch {
    /* тост показан */
  } finally {
    pinning.value = ''
  }
}

async function checkOne(domain) {
  checking.value = domain
  try {
    await checkAction.mutateAsync(domain)
    notify('Проверка маршрута запущена')
  } catch {
    /* тост показан */
  } finally {
    checking.value = ''
  }
}

async function checkAll() {
  try {
    await checkAction.mutateAsync('')
    notify('Проверка всех маршрутов запущена')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Маршрутизация доменов"
    description="Один список: маршрут выбирается автоматически по скорости. Перепроверка раз в 10 минут."
  >
    <template #icon>
      <ListFilter class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex flex-wrap justify-end gap-2">
        <Button
          size="sm"
          variant="outline"
          :loading="checkAction.isLoading.value"
          :disabled="checkAction.isLoading.value || !autoDomains.length"
          @click="checkAll"
        >
          <Activity class="h-3.5 w-3.5" />
          Проверить все
        </Button>
        <Button size="sm" @click="openAdd(false)">
          <Plus class="h-3.5 w-3.5" />
          Добавить домен
        </Button>
      </div>

      <details open class="rounded-lg border bg-muted/30 px-3 py-2">
        <summary class="flex cursor-pointer flex-wrap items-center gap-2 text-sm">
          <Badge variant="secondary">AUTO</Badge>
          <span class="font-medium">Домены с авто-маршрутом</span>
          <span class="text-xs text-muted-foreground">{{ autoDomains.length }}</span>
        </summary>
        <div class="mt-3 space-y-2">
          <p class="text-xs text-muted-foreground">
            Проверяем напрямую, через Zapret, WireGuard и VLESS — выбираем самый быстрый
            рабочий путь. Кнопка с булавкой закрепляет домен за VPN, если прямой ответ 200
            на самом деле является региональной заглушкой. Поддомены учитываются
            автоматически.
          </p>
          <ul v-if="autoDomains.length" class="divide-y rounded-lg border">
            <li
              v-for="item in autoDomains"
              :key="item"
              class="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-sm"
            >
              <span class="min-w-0 break-all font-mono text-xs">{{ item }}</span>
              <div class="flex items-center gap-1.5">
                <Badge
                  :variant="routeBadge(item).variant"
                  :title="routeTitle(item)"
                  class="whitespace-nowrap"
                >
                  {{ routeBadge(item).text }}
                </Badge>
                <Button
                  size="sm"
                  variant="ghost"
                  :loading="pinning === item"
                  :disabled="!!checking || !!removing || !!pinning"
                  :aria-label="`Всегда через VPN: ${item}`"
                  :title="
                    isVpnOnly(item)
                      ? 'Вернуть авто-выбор маршрута'
                      : 'Всегда через VPN, даже если прямой ответ 200'
                  "
                  @click="toggleVpnOnly(item)"
                >
                  <Pin class="h-3.5 w-3.5" :class="isVpnOnly(item) ? 'fill-current' : ''" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  :loading="checking === item"
                  :disabled="!!checking || !!removing || !!pinning"
                  :aria-label="`Проверить ${item}`"
                  :title="`Проверить ${item}`"
                  @click="checkOne(item)"
                >
                  <Activity class="h-3.5 w-3.5" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  :loading="removing === `auto:${item}`"
                  :disabled="!!removing || !!checking || !!pinning"
                  :aria-label="`Удалить ${item}`"
                  :title="`Удалить ${item}`"
                  @click="remove(item, false)"
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </Button>
              </div>
            </li>
          </ul>
          <p v-else class="text-xs text-muted-foreground">Пока пусто. Добавьте домен.</p>
        </div>
      </details>

      <details class="rounded-lg border bg-muted/30 px-3 py-2">
        <summary class="flex cursor-pointer flex-wrap items-center gap-2 text-sm">
          <Badge variant="outline">DIRECT</Badge>
          <span class="font-medium">Напрямую (OpenVPN / LAN)</span>
          <span class="text-xs text-muted-foreground">{{ directDomains.length }}</span>
        </summary>
        <div class="mt-3 space-y-2">
          <p class="text-xs text-muted-foreground">
            Всегда мимо VPN — через системные маршруты OpenVPN или локальной сети.
            Имеет приоритет над авто-маршрутом. Поддомены учитываются.
          </p>
          <div class="flex justify-end">
            <Button size="sm" variant="outline" @click="openAdd(true)">
              <Plus class="h-3.5 w-3.5" />
              Добавить напрямую
            </Button>
          </div>
          <ul v-if="directDomains.length" class="divide-y rounded-lg border">
            <li
              v-for="item in directDomains"
              :key="item"
              class="flex items-center justify-between px-3 py-1.5 text-sm"
            >
              <span class="font-mono text-xs">{{ item }}</span>
              <Button
                size="sm"
                variant="ghost"
                :loading="removing === `direct:${item}`"
                :disabled="!!removing || !!checking"
                :aria-label="`Удалить ${item}`"
                :title="`Удалить ${item}`"
                @click="remove(item, true)"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </Button>
            </li>
          </ul>
          <p v-else class="text-xs text-muted-foreground">Пока пусто.</p>
        </div>
      </details>
    </div>

    <Dialog
      :open="addOpen"
      title="Добавить домен"
      :description="
        addDirect
          ? 'Домен всегда будет идти напрямую, мимо VPN.'
          : 'Маршрут для домена будет выбран автоматически по скорости.'
      "
      @close="addOpen = false"
    >
      <DomainAddForm
        :submit="submitAdd"
        :loading="addAction.isLoading.value"
        :disabled="!!removing"
      />
    </Dialog>
  </Card>
</template>
