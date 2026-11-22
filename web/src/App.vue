<script setup>
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue'
import { BookOpen, RefreshCw, ShieldCheck } from 'lucide-vue-next'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { api, store } from '@/lib/store'
import { useStatusQuery } from '@/lib/queries'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Tabs from '@/components/ui/Tabs.vue'
import DocsDialog from '@/components/DocsDialog.vue'
import PluginSetupDialog from '@/components/PluginSetupDialog.vue'
import StatusDialog from '@/components/StatusDialog.vue'
import PluginDisabledView from '@/views/PluginDisabledView.vue'
import Toast from '@/components/Toast.vue'

const router = useRouter()
const route = useRoute()
const statusQuery = useStatusQuery()
const s = computed(() => statusQuery.data.value)
const offline = computed(() => !!statusQuery.error.value)
const online = computed(() => !offline.value && !!s.value && (s.value.wgUp || s.value.vlessUp))
const docsOpen = ref(false)
const statusOpen = ref(false)
const firstRunOpen = ref(false)
const firstRunDismissed = ref(false)
const refreshing = ref(false)
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
  const items = [
    { label: 'Обзор', to: '/overview' },
    { label: 'Каналы', to: '/channels' },
    { label: 'Сайты', to: '/sites' },
  ]
  if (plugins.telegram) items.push({ label: 'Telegram', to: '/telegram' })
  if (plugins.zapret && data?.zapret?.supported) items.push({ label: 'Обход DPI', to: '/dpi' })
  if (plugins.localDomains) items.push({ label: 'Локальные домены', to: '/domains/local' })
  items.push({ label: 'Настройки', to: '/settings' })
  return items.map((item) => ({
    ...item,
    active: route.path === item.to || route.path.startsWith(`${item.to}/`),
  }))
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

watch(
  () => route.query.docs,
  (value) => {
    if (value === '1') docsOpen.value = true
  },
  { immediate: true },
)

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

function closeDocs() {
  docsOpen.value = false
  if (route.query.docs) {
    const query = { ...route.query }
    delete query.docs
    router.replace({ path: route.path, query })
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

function routeAllowed(value) {
  const plugin = value.meta?.plugin
  if (!plugin) return true
  if (plugin === 'zapret') {
    return !!s.value?.plugins?.zapret && s.value?.zapret?.supported !== false
  }
  return !!s.value?.plugins?.[plugin]
}

onMounted(() => {
  window.addEventListener('wpc:status-refresh', refetchStatus)
  document.addEventListener('visibilitychange', onVisibilityChange)
  // У @pinia/colada нет refetchInterval — держим polling сами, иначе
  // здоровье каналов, прогресс загрузок и логи замирают без действий.
  pollTimer = window.setInterval(pollStatus, 5000)
})

onUnmounted(() => {
  window.removeEventListener('wpc:status-refresh', refetchStatus)
  document.removeEventListener('visibilitychange', onVisibilityChange)
  if (pollTimer !== null) window.clearInterval(pollTimer)
})
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
          <Button variant="ghost" size="sm" class="h-8 px-2" @click="statusOpen = true">
            <Badge :variant="online ? 'success' : 'destructive'">
              {{ online ? 'VPN активен' : 'VPN не активен' }}
            </Badge>
          </Button>
          <Button
            variant="outline"
            size="icon"
            aria-label="Обновить состояние"
            title="Обновить состояние"
            :loading="refreshing"
            @click="refreshStatus"
          >
            <RefreshCw class="h-3.5 w-3.5" />
          </Button>
          <Button variant="outline" size="sm" @click="docsOpen = true">
            <BookOpen class="h-3.5 w-3.5" />
            Документация
          </Button>
        </div>
      </div>
      <Tabs :tabs="tabs" />
    </header>

    <main class="container space-y-4 py-5">
      <div
        v-if="store.offline"
        class="flex flex-wrap items-center gap-3 rounded-xl border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      >
        <span>Панель недоступна. Проверьте, что менеджер запущен.</span>
        <Button size="sm" variant="outline" :loading="refreshing" @click="refreshStatus">
          Повторить
        </Button>
      </div>

      <template v-if="s">
        <RouterView v-slot="{ Component, route: currentRoute }">
          <Transition name="page" mode="out-in">
            <component
              :is="Component"
              v-if="routeAllowed(currentRoute)"
              :key="currentRoute.matched[0]?.path || currentRoute.path"
              :s="s"
            />
            <PluginDisabledView
              v-else
              :key="currentRoute.matched[0]?.path || currentRoute.path"
              :s="s"
              :plugin="currentRoute.meta?.plugin || ''"
            />
          </Transition>
        </RouterView>
      </template>

      <div v-else class="py-20 text-center text-sm text-muted-foreground">
        Загружаю состояние…
      </div>
    </main>

    <DocsDialog :open="docsOpen" @close="closeDocs" />
    <StatusDialog v-if="s" :open="statusOpen" :s="s" @close="statusOpen = false" />
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
