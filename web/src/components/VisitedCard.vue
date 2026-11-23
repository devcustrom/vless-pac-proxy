<script setup>
import { computed, reactive, ref, watch } from 'vue'
import { Check, ChevronLeft, ChevronRight, Eye, Wand2 } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { useRoute, useRouter } from 'vue-router'
import { api, notify, post, refresh } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Select from '@/components/ui/Select.vue'
import Switch from '@/components/ui/Switch.vue'
import OverflowMenu from '@/components/ui/OverflowMenu.vue'
import StrategyDialog from '@/components/StrategyDialog.vue'
import { normalizeDomainQuery } from '@/lib/validators'

const props = defineProps({ s: { type: Object, required: true } })

const PAGE_SIZE = 20
const page = ref(1)
const vpnBusy = ref('')
const routeOverrides = reactive({})
const checks = reactive({})
const bulk = reactive({ running: false, done: 0, total: 0 })
const filter = ref('all')
const query = ref('')
const strategyFor = ref('')
const z = computed(() => props.s.zapret || {})
const canGenerate = computed(
  () =>
    !!props.s.plugins?.zapret &&
    !!z.value.supported &&
    !!z.value.bundleInstalled &&
    !!z.value.blockcheck,
)

const FILTERS = [
  { value: 'all', label: 'Все' },
  { value: 'vpn', label: 'Через VPN' },
  { value: 'direct', label: 'Напрямую' },
  { value: 'needs', label: 'Нужен VPN' },
  { value: 'unchecked', label: 'Без проверки' },
]

const router = useRouter()
const route = useRoute()
if (FILTERS.some((item) => item.value === route.query.filter)) {
  filter.value = String(route.query.filter)
}
if (typeof route.query.q === 'string') query.value = route.query.q

watch([filter, query], ([nextFilter, nextQuery]) => {
  const params = {}
  if (nextFilter !== 'all') params.filter = nextFilter
  if (nextQuery.trim()) params.q = nextQuery.trim()
  router.replace({ path: '/sites', query: params })
})

const VERDICTS = {
  'vpn-helps': 'нужен VPN',
  'direct-only': 'VPN только ухудшает',
  'both-ok': 'работает напрямую',
  'zapret-helps': 'работает через Zapret',
  challenge: 'анти-бот / 403',
  'blocked-everywhere': 'не открывается',
}

const items = computed(() => {
  const cutoff = Math.floor(Date.now() / 1000) - 86_400
  return (props.s.visited || []).filter((item) => item.last >= cutoff)
})

const latestChecks = computed(() => {
  const map = {}
  for (const item of props.s.checks || []) {
    if (!map[item.domain]) map[item.domain] = item
  }
  return map
})

const filtered = computed(() => {
  const q = normalizeDomainQuery(query.value).trim().toLowerCase()
  return items.value.filter((item) => {
    if (q && !item.domain.toLowerCase().includes(q)) return false
    const check = latestChecks.value[item.domain]
    if (filter.value === 'vpn') return !!item.route
    if (filter.value === 'direct') return !item.route
    if (filter.value === 'needs') return check?.verdict === 'vpn-helps'
    if (filter.value === 'unchecked') return !item.checked
    return true
  })
})

const total = computed(() => filtered.value.length)
const pageCount = computed(() => Math.max(1, Math.ceil(filtered.value.length / PAGE_SIZE)))
const visible = computed(() => {
  const start = (page.value - 1) * PAGE_SIZE
  return filtered.value.slice(start, start + PAGE_SIZE)
})

const anyChecking = computed(
  () => bulk.running || Object.values(checks).some((item) => item?.busy),
)

watch([pageCount, filter, query], () => {
  page.value = 1
})
watch(pageCount, (count) => {
  if (page.value > count) page.value = count
})

function onQueryPaste(event) {
  const text = event.clipboardData?.getData('text') || ''
  const cleaned = normalizeDomainQuery(text)
  if (!cleaned) return
  event.preventDefault()
  query.value = cleaned
}

function routeLabel(item) {
  if (!item.route) return 'напрямую'
  return item.route === 'vless' ? 'VPN · VLESS' : 'VPN · WireGuard'
}

function checkFor(domain) {
  return checks[domain] || latestChecks.value[domain] || null
}

function codeVariant(code) {
  return code && code !== '000' && code !== '403' && code !== '451' ? 'success' : 'destructive'
}

function codeText(code) {
  return code === '000' ? 'нет' : code
}

function zapretText(item) {
  if (!item?.zapretSupported) return 'нет'
  if (!item?.zapretRunning) return 'выкл'
  return codeText(item.zapret)
}

function zapretVariant(item) {
  if (!item?.zapretSupported) return 'outline'
  if (!item?.zapretRunning) return 'secondary'
  return codeVariant(item.zapret)
}

async function runCheck(domain, refreshAfter = true, quiet = false) {
  checks[domain] = { ...(checks[domain] || {}), busy: true, error: null }
  try {
    const result = await api('/api/check', { domain })
    checks[domain] = { ...result, busy: false }
    if (refreshAfter) await refresh()
  } catch (error) {
    checks[domain] = { busy: false, error: error.message }
    if (!quiet) notify(error.message, 'error')
  }
}

async function checkAll() {
  if (bulk.running || !items.value.length) return
  bulk.running = true
  bulk.done = 0
  bulk.total = items.value.length
  const queue = items.value.map((item) => item.domain)
  const worker = async () => {
    while (queue.length) {
      const domain = queue.shift()
      await runCheck(domain, false, true)
      bulk.done += 1
    }
  }
  try {
    await Promise.all([worker(), worker(), worker()])
    await refresh()
  } finally {
    bulk.running = false
  }
}

function vpnEnabled(item) {
  return item.domain in routeOverrides ? routeOverrides[item.domain] : !!item.route
}

async function toggleVpn(item, enabled) {
  routeOverrides[item.domain] = enabled
  vpnBusy.value = item.domain
  try {
    await post('/api/extra', {
      action: 'route',
      domain: item.domain,
      enabled,
    })
    notify(`${item.domain}: ${enabled ? 'через VPN' : 'напрямую'}`)
    await refresh()
    setTimeout(() => {
      delete routeOverrides[item.domain]
    }, 6000)
  } catch {
    delete routeOverrides[item.domain]
  } finally {
    vpnBusy.value = ''
  }
}
</script>

<template>
  <Card
    title="Недавние сайты"
    description="Уникальные домены за последние 24 часа. Проверьте сайт или весь список; фоновая задача проверяет новые автоматически."
  >
    <template #icon>
      <Eye class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            :loading="bulk.running"
            :disabled="!items.length || bulk.running"
            @click="checkAll"
          >
            <Check class="h-3.5 w-3.5" />
            Проверить все
          </Button>
          <RouterLink
            to="/domains/routing"
            class="inline-flex h-8 items-center rounded-md border border-input bg-background px-3 text-xs font-medium shadow-sm transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            Маршрутизация
          </RouterLink>
        </div>
        <span v-if="bulk.running" class="text-xs text-muted-foreground">
          проверено {{ bulk.done }} из {{ bulk.total }}…
        </span>
        <RouterLink v-else to="/settings" class="text-xs text-muted-foreground hover:text-foreground">
          автопроверка {{ s.autoCheckSites ? 'включена' : 'выключена' }}
        </RouterLink>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <Input
          v-model="query"
          placeholder="Поиск по домену или ссылка из браузера…"
          class="h-8 min-w-48 flex-1"
          autocomplete="off"
          @paste="onQueryPaste"
          @blur="query = normalizeDomainQuery(query) || query"
        />
        <div class="w-48">
          <Select v-model="filter" :options="FILTERS" />
        </div>
      </div>

      <ul v-if="visible.length" class="divide-y rounded-lg border">
        <li
          v-for="item in visible"
          :key="item.domain"
          class="px-3 py-2 text-sm"
        >
          <div class="space-y-2">
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span class="min-w-0 break-all text-sm font-medium">{{ item.domain }}</span>
              <span class="text-xs text-muted-foreground">×{{ item.count }}</span>
              <Badge :variant="item.route ? 'success' : 'outline'">
                {{ routeLabel(item) }}
              </Badge>
            </div>
            <div class="flex flex-wrap items-center gap-1.5">
              <OverflowMenu label="Действия с доменом">
                <button
                  type="button"
                  class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
                  :disabled="anyChecking || !!vpnBusy"
                  @click="runCheck(item.domain)"
                >
                  <Check class="h-3.5 w-3.5" />
                  {{ checks[item.domain]?.busy ? 'Проверяю…' : 'Проверить' }}
                </button>
                <button
                  v-if="canGenerate"
                  type="button"
                  class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
                  :disabled="anyChecking || !!vpnBusy || bulk.running"
                  @click="strategyFor = item.domain"
                >
                  <Wand2 class="h-3.5 w-3.5" />
                  Подобрать стратегии
                </button>
              </OverflowMenu>
              <div
                class="flex items-center gap-1.5 rounded-md border px-2 py-1 text-xs"
                :class="vpnEnabled(item) ? 'border-primary/40 bg-primary/5' : 'bg-background'"
                title="Маршрутизировать этот домен через VPN"
              >
                <Switch
                  aria-label="VPN"
                  :model-value="vpnEnabled(item)"
                  :disabled="!!vpnBusy || bulk.running"
                  @update:model-value="toggleVpn(item, $event)"
                />
                <span>VPN</span>
                <span v-if="vpnBusy === item.domain" class="text-muted-foreground">…</span>
              </div>
            </div>
            <div v-if="checkFor(item.domain)?.busy" class="text-xs text-muted-foreground">
              проверяю…
            </div>
            <div v-else-if="checkFor(item.domain)" class="flex flex-wrap items-center gap-1.5 text-xs">
              <Badge :variant="codeVariant(checkFor(item.domain).direct)">
                напрямую {{ codeText(checkFor(item.domain).direct) }}
              </Badge>
              <Badge :variant="codeVariant(checkFor(item.domain).vpn)">
                VPN {{ codeText(checkFor(item.domain).vpn) }}
              </Badge>
              <Badge :variant="zapretVariant(checkFor(item.domain))">
                Zapret {{ zapretText(checkFor(item.domain)) }}
              </Badge>
              <span class="text-muted-foreground">
                {{ VERDICTS[checkFor(item.domain).verdict] || checkFor(item.domain).verdict }}
              </span>
              <span v-if="checkFor(item.domain).error" class="text-destructive">
                {{ checkFor(item.domain).error }}
              </span>
            </div>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Пока пусто — список наполнится, когда включён системный прокси.
      </p>

      <div
        v-if="filtered.length > PAGE_SIZE"
        class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground"
      >
        <span>
          всего {{ total }} · страница {{ page }} из {{ pageCount }}
        </span>
        <div class="flex gap-1">
          <Button
            size="sm"
            variant="outline"
            :disabled="page <= 1"
            @click="page--"
          >
            <ChevronLeft class="h-3.5 w-3.5" />
            Назад
          </Button>
          <Button
            size="sm"
            variant="outline"
            :disabled="page >= pageCount"
            @click="page++"
          >
            Вперёд
            <ChevronRight class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </div>

    <StrategyDialog
      :open="!!strategyFor"
      :domain="strategyFor"
      :s="s"
      @close="strategyFor = ''"
    />
  </Card>
</template>
