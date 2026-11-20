<script setup>
import { computed, reactive, ref, watch } from 'vue'
import { Check, ChevronLeft, ChevronRight, Eye, Plus } from 'lucide-vue-next'
import { api, notify, post, refresh } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({ s: { type: Object, required: true } })

const PAGE_SIZE = 20
const page = ref(1)
const adding = ref('')
const checks = reactive({})
const bulk = reactive({ running: false, done: 0, total: 0 })

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
const total = computed(() => props.s.visitedTotal ?? items.value.length)
const pageCount = computed(() => Math.max(1, Math.ceil(items.value.length / PAGE_SIZE)))
const visible = computed(() => {
  const start = (page.value - 1) * PAGE_SIZE
  return items.value.slice(start, start + PAGE_SIZE)
})

const latestChecks = computed(() => {
  const map = {}
  for (const item of props.s.checks || []) {
    if (!map[item.domain]) map[item.domain] = item
  }
  return map
})

const anyChecking = computed(
  () => bulk.running || Object.values(checks).some((item) => item?.busy),
)

watch(pageCount, (count) => {
  if (page.value > count) page.value = count
})

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

async function addToVpn(domain) {
  adding.value = domain
  try {
    await post('/api/extra', { action: 'add', domain })
    notify(domain + ' добавлен в VPN-список')
  } catch {
    /* тост уже показан */
  } finally {
    adding.value = ''
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
        <span v-if="bulk.running" class="text-xs text-muted-foreground">
          проверено {{ bulk.done }} из {{ bulk.total }}…
        </span>
        <span v-else class="text-xs text-muted-foreground">
          автопроверка {{ s.autoCheckSites ? 'включена' : 'выключена' }}
        </span>
      </div>

      <ul v-if="visible.length" class="divide-y rounded-lg border">
        <li
          v-for="item in visible"
          :key="item.domain"
          class="px-3 py-2 text-sm"
        >
          <div class="flex flex-wrap items-center gap-2">
            <span class="min-w-0 flex-1 truncate">
              {{ item.domain }}
              <span class="text-xs text-muted-foreground">×{{ item.count }}</span>
            </span>
            <Button
              size="sm"
              variant="outline"
              :loading="!!checks[item.domain]?.busy"
              :disabled="anyChecking || !!adding"
              @click="runCheck(item.domain)"
            >
              <Check class="h-3.5 w-3.5" />
              Проверить
            </Button>
            <Button
              size="sm"
              variant="outline"
              :loading="adding === item.domain"
              :disabled="!!adding || bulk.running"
              @click="addToVpn(item.domain)"
            >
              <Plus class="h-3.5 w-3.5" />
              через VPN
            </Button>
          </div>
          <div v-if="checkFor(item.domain)?.busy" class="mt-1 text-xs text-muted-foreground">
            проверяю…
          </div>
          <div v-else-if="checkFor(item.domain)" class="mt-1 flex flex-wrap items-center gap-1.5 text-xs">
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
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Пока пусто — список наполнится, когда включён системный прокси.
      </p>

      <div
        v-if="items.length > PAGE_SIZE"
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
  </Card>
</template>
