<script setup>
import { computed, onMounted, ref } from 'vue'
import { BookOpen, RefreshCw, ShieldCheck } from 'lucide-vue-next'
import { store, refresh, startPolling } from '@/lib/store'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import OnboardingCard from '@/components/OnboardingCard.vue'
import ChannelsCard from '@/components/ChannelsCard.vue'
import WireGuardCard from '@/components/WireGuardCard.vue'
import VlessCard from '@/components/VlessCard.vue'
import PoolCard from '@/components/PoolCard.vue'
import LocalDomainsCard from '@/components/LocalDomainsCard.vue'
import ListsCard from '@/components/ListsCard.vue'
import CheckCard from '@/components/CheckCard.vue'
import VisitedCard from '@/components/VisitedCard.vue'
import ExtraDomainsCard from '@/components/ExtraDomainsCard.vue'
import UpdateCard from '@/components/UpdateCard.vue'
import DocsDialog from '@/components/DocsDialog.vue'
import Toast from '@/components/Toast.vue'

const s = computed(() => store.status)
const pacUrl = computed(() => `${window.location.origin}/proxy.pac`)
const online = computed(() => !!s.value && (s.value.wgUp || s.value.vlessUp))
const docsOpen = ref(false)

function closeDocs() {
  docsOpen.value = false
  if (window.location.hash === '#docs') {
    history.replaceState(null, '', window.location.pathname + window.location.search)
  }
}

onMounted(() => {
  startPolling()
  if (window.location.hash === '#docs') docsOpen.value = true
  window.addEventListener('hashchange', () => {
    if (window.location.hash === '#docs') docsOpen.value = true
  })
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
          <p class="text-xs text-muted-foreground">WireGuard + VLESS · умная маршрутизация по доменам</p>
        </div>
        <div class="ml-auto flex flex-wrap items-center gap-2">
          <Badge :variant="online ? 'success' : 'destructive'">
            {{ online ? 'VPN активен' : 'VPN не активен' }}
          </Badge>
          <a
            :href="pacUrl"
            target="_blank"
            class="hidden text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline sm:inline"
          >
            proxy.pac
          </a>
          <Button variant="outline" size="sm" @click="refresh">
            <RefreshCw class="h-3.5 w-3.5" />
            Обновить
          </Button>
          <Button variant="outline" size="sm" @click="docsOpen = true">
            <BookOpen class="h-3.5 w-3.5" />
            Документация
          </Button>
        </div>
      </div>
    </header>

    <main class="container space-y-4 py-5">
      <div
        v-if="store.offline"
        class="rounded-xl border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      >
        Панель недоступна. Проверьте, что менеджер запущен, и обновите страницу.
      </div>

      <template v-if="s">
        <OnboardingCard :s="s" />
        <div class="grid items-start gap-4 lg:grid-cols-2">
          <div class="space-y-4">
            <ChannelsCard :s="s" />
            <WireGuardCard :s="s" />
            <VlessCard :s="s" />
            <PoolCard :s="s" />
            <LocalDomainsCard :s="s" />
          </div>
          <div class="space-y-4">
            <UpdateCard />
            <ListsCard :s="s" />
            <CheckCard :s="s" />
            <VisitedCard :s="s" />
            <ExtraDomainsCard :s="s" />
          </div>
        </div>
      </template>

      <div v-else class="py-20 text-center text-sm text-muted-foreground">
        Загружаю состояние…
      </div>
    </main>

    <DocsDialog :open="docsOpen" @close="closeDocs" />
    <Toast />
  </div>
</template>
