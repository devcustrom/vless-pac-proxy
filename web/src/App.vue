<script setup>
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue'
import { BookOpen, ListFilter, RefreshCw, ShieldCheck } from 'lucide-vue-next'
import { api, store } from '@/lib/store'
import { useStatusQuery } from '@/lib/queries'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Tabs from '@/components/ui/Tabs.vue'
import OnboardingCard from '@/components/OnboardingCard.vue'
import ChannelsCard from '@/components/ChannelsCard.vue'
import WireGuardCard from '@/components/WireGuardCard.vue'
import VlessCard from '@/components/VlessCard.vue'
import PoolCard from '@/components/PoolCard.vue'
import TelegramCard from '@/components/TelegramCard.vue'
import ZapretCard from '@/components/ZapretCard.vue'
import LocalDomainsCard from '@/components/LocalDomainsCard.vue'
import RoutingDialog from '@/components/RoutingDialog.vue'
import PluginSetupDialog from '@/components/PluginSetupDialog.vue'
import ListsCard from '@/components/ListsCard.vue'
import CheckCard from '@/components/CheckCard.vue'
import VisitedCard from '@/components/VisitedCard.vue'
import UpdateCard from '@/components/UpdateCard.vue'
import SettingsCard from '@/components/SettingsCard.vue'
import DocsDialog from '@/components/DocsDialog.vue'
import Toast from '@/components/Toast.vue'

const ALL_TABS = {
  overview: { value: 'overview', label: 'Обзор' },
  configs: { value: 'configs', label: 'Конфиги' },
  telegram: { value: 'telegram', label: 'Telegram' },
  zapret: { value: 'zapret', label: 'Zapret' },
  domains: { value: 'domains', label: 'Домены' },
  settings: { value: 'settings', label: 'Настройки' },
}

const statusQuery = useStatusQuery()
const s = computed(() => statusQuery.data.value)
const offline = computed(() => !!statusQuery.error.value)
const online = computed(() => !offline.value && !!s.value && (s.value.wgUp || s.value.vlessUp))
const docsOpen = ref(false)
const routingOpen = ref(false)
const firstRunOpen = ref(false)
const firstRunDismissed = ref(false)
const refreshing = ref(false)
const tab = ref('overview')
let pollTimer = null
let reloading = false

// Если вкладка была открыта до обновления бинарника, её JS-бандл старее
// backend. Перезагружаем страницу один раз, чтобы не показывать старые
// переключатели после апдейта.
watch(
  () => s.value?.version,
  (version) => {
    if (version && version !== __WPC_WEB_VERSION__ && !reloading) {
      reloading = true
      window.location.reload()
    }
  },
)

const tabs = computed(() => {
  const data = s.value
  const plugins = data?.plugins || {}
  const items = [ALL_TABS.overview, ALL_TABS.configs]
  if (plugins.telegram) items.push(ALL_TABS.telegram)
  if (plugins.zapret && data?.zapret?.supported) items.push(ALL_TABS.zapret)
  if (plugins.localDomains) items.push(ALL_TABS.domains)
  items.push(ALL_TABS.settings)
  return items
})

watch(tabs, (items) => {
  if (!items.some((item) => item.value === tab.value)) {
    tab.value = 'overview'
    history.replaceState(null, '', '#overview')
  }
})

watchEffect(() => {
  // Ошибка важнее старых данных: Pinia Colada сохраняет последний успешный
  // snapshot, но если менеджер умер, нельзя показывать «VPN активен».
  store.offline = offline.value
  if (statusQuery.data.value) {
    store.status = statusQuery.data.value
    const data = statusQuery.data.value
    if (!data.onboardingDone && !firstRunDismissed.value) {
      if (data.plugins?.localDomains) {
        // Плагин уже включён — мастер первого запуска не нужен.
        firstRunDismissed.value = true
        api('/api/onboarding', {}).catch(() => {})
      } else {
        firstRunOpen.value = true
      }
    }
  }
})

async function refreshStatus() {
  refreshing.value = true
  try {
    await statusQuery.refetch()
  } finally {
    refreshing.value = false
  }
}

async function dismissFirstRun() {
  firstRunOpen.value = false
  firstRunDismissed.value = true
  await api('/api/onboarding', {}).catch(() => {})
}

function onPluginEnabled() {
  firstRunOpen.value = false
  firstRunDismissed.value = true
}

function syncTabFromHash() {
  const hash = window.location.hash.replace('#', '')
  if (tabs.value.some((item) => item.value === hash)) tab.value = hash
  if (hash === 'docs') docsOpen.value = true
}

function setTab(value) {
  tab.value = value
  history.replaceState(null, '', `#${value}`)
}

function closeDocs() {
  docsOpen.value = false
  if (window.location.hash === '#docs') {
    history.replaceState(null, '', window.location.pathname + window.location.search)
  }
}

function refetchStatus() {
  statusQuery.refetch().catch(() => {
    /* следующий polling попробует снова */
  })
}

function pollStatus() {
  if (!document.hidden) refetchStatus()
}

function onVisibilityChange() {
  if (!document.hidden) refetchStatus()
}

onMounted(() => {
  syncTabFromHash()
  window.addEventListener('hashchange', syncTabFromHash)
  window.addEventListener('wpc:status-refresh', refetchStatus)
  document.addEventListener('visibilitychange', onVisibilityChange)
  // У @pinia/colada нет refetchInterval — держим polling сами, иначе
  // здоровье каналов, прогресс загрузок и логи замирают без действий.
  pollTimer = window.setInterval(pollStatus, 5000)
})

onUnmounted(() => {
  window.removeEventListener('hashchange', syncTabFromHash)
  window.removeEventListener('wpc:status-refresh', refetchStatus)
  document.removeEventListener('visibilitychange', onVisibilityChange)
  if (pollTimer !== null) window.clearInterval(pollTimer)
})

watch(s, () => syncTabFromHash())
</script>

<template>
  <div class="min-h-screen bg-muted/30">
    <header class="sticky top-0 z-10 border-b bg-background/85 backdrop-blur">
      <div class="container flex flex-wrap items-center gap-3 py-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-primary text-primary-foreground">
          <ShieldCheck class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <h1 class="text-lg font-semibold leading-tight">wireproxy-control</h1>
          <p class="text-xs text-muted-foreground">WireGuard + Xray · умная маршрутизация по доменам</p>
        </div>
        <div class="ml-auto flex flex-wrap items-center gap-2">
          <Badge :variant="online ? 'success' : 'destructive'">
            {{ online ? 'VPN активен' : 'VPN не активен' }}
          </Badge>
          <Button variant="outline" size="sm" :loading="refreshing" @click="refreshStatus">
            <RefreshCw class="h-3.5 w-3.5" />
            Обновить
          </Button>
          <Button variant="outline" size="sm" @click="docsOpen = true">
            <BookOpen class="h-3.5 w-3.5" />
            Документация
          </Button>
        </div>
      </div>
      <Tabs :model-value="tab" :tabs="tabs" @update:model-value="setTab" />
    </header>

    <main class="container space-y-4 py-5">
      <div
        v-if="store.offline"
        class="rounded-xl border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      >
        Панель недоступна. Проверьте, что менеджер запущен, и обновите страницу.
      </div>

      <template v-if="s">
        <template v-if="tab === 'overview'">
          <div class="flex justify-end">
            <Button variant="outline" size="sm" @click="routingOpen = true">
              <ListFilter class="h-3.5 w-3.5" />
              Маршрутизация доменов
            </Button>
          </div>
          <OnboardingCard :s="s" />
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <ChannelsCard :s="s" />
              <ListsCard :s="s" />
              <CheckCard :s="s" />
            </div>
            <div class="space-y-4">
              <UpdateCard />
              <VisitedCard :s="s" />
            </div>
          </div>
        </template>

        <template v-else-if="tab === 'configs'">
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <WireGuardCard :s="s" />
              <VlessCard :s="s" />
            </div>
            <div class="space-y-4">
              <PoolCard :s="s" />
            </div>
          </div>
        </template>

        <template v-else-if="tab === 'telegram'">
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <TelegramCard :s="s" />
            </div>
          </div>
        </template>

        <template v-else-if="tab === 'zapret'">
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <ZapretCard :s="s" />
            </div>
          </div>
        </template>

        <template v-else-if="tab === 'domains'">
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <LocalDomainsCard :s="s" />
            </div>
          </div>
        </template>

        <template v-else>
          <div class="grid items-start gap-4 lg:grid-cols-2">
            <div class="space-y-4">
              <SettingsCard :s="s" />
            </div>
            <div class="space-y-4">
              <UpdateCard />
            </div>
          </div>
        </template>
      </template>

      <div v-else class="py-20 text-center text-sm text-muted-foreground">
        Загружаю состояние…
      </div>
    </main>

    <DocsDialog :open="docsOpen" @close="closeDocs" />
    <RoutingDialog v-if="s" :open="routingOpen" :s="s" @close="routingOpen = false" />
    <PluginSetupDialog
      v-if="s"
      :open="firstRunOpen"
      :s="s"
      first-run
      @close="dismissFirstRun"
      @enabled="onPluginEnabled"
    />
    <Toast />
  </div>
</template>
