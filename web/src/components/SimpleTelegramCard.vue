<script setup>
import { computed, ref } from 'vue'
import { Copy, ExternalLink, Send, Settings2 } from 'lucide-vue-next'
import { api, notify, post } from '@/lib/store'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({ s: { type: Object, required: true } })
const emit = defineEmits(['advanced'])

const t = computed(() => props.s.telegram || {})
const enabled = computed(() => !!props.s.plugins?.telegram)
const running = computed(() => !!t.value.running)
const installed = computed(() => !!t.value.installed)
const stats = computed(() => t.value.stats || {})
const busy = ref('')

const orderLabels = {
  direct: 'напрямую',
  wg: 'WireGuard',
  vless: 'VLESS',
}
const orderText = computed(() =>
  (t.value.order || []).map((item) => orderLabels[item] || item).join(' → '),
)
const lastPath = computed(() => {
  const path = t.value.upstream?.last_path
  return path ? orderLabels[path] || path : '—'
})

function fmtBytes(value) {
  const n = Number(value || 0)
  if (n < 1024) return `${n} Б`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} КБ`
  return `${(n / 1024 / 1024).toFixed(1)} МБ`
}

async function getLink() {
  const result = await api('/api/telegram/link', {})
  return result.link
}

async function openTelegram() {
  busy.value = 'open'
  try {
    const link = await getLink()
    window.location.href = link
  } catch (error) {
    notify(error.message || 'Не удалось получить ссылку', 'error')
  } finally {
    busy.value = ''
  }
}

async function copyLink() {
  busy.value = 'copy'
  try {
    const link = await getLink()
    await navigator.clipboard.writeText(link)
    notify('Ссылка на Telegram-прокси скопирована')
  } catch (error) {
    notify(error.message || 'Не удалось получить ссылку', 'error')
  } finally {
    busy.value = ''
  }
}

async function start() {
  busy.value = 'start'
  try {
    await post('/api/telegram', { action: 'restart' })
    notify('Telegram-прокси запускается')
  } catch {
    /* тост показан */
  } finally {
    busy.value = ''
  }
}
</script>

<template>
  <div
    v-if="enabled"
    class="w-full rounded-2xl border bg-background p-4 shadow-sm sm:p-5"
  >
    <div class="flex flex-wrap items-center gap-3">
      <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
        <Send class="h-5 w-5" />
      </div>
      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-sm font-medium">Telegram-прокси</span>
          <Badge :variant="running ? 'success' : installed ? 'secondary' : 'warning'">
            {{ running ? 'работает' : installed ? 'остановлен' : 'не установлен' }}
          </Badge>
        </div>
        <p class="mt-0.5 text-xs text-muted-foreground">
          {{ t.host }}:{{ t.port }}
          <template v-if="orderText"> · {{ orderText }}</template>
        </p>
      </div>
      <Button
        v-if="running"
        size="sm"
        :loading="busy === 'open'"
        :disabled="!!busy"
        @click="openTelegram"
      >
        <ExternalLink class="h-3.5 w-3.5" />
        Открыть
      </Button>
    </div>

    <div v-if="running" class="mt-3 grid grid-cols-3 gap-2 text-xs">
      <div class="rounded-lg border px-2 py-2">
        <div class="text-muted-foreground">Активных</div>
        <div class="font-medium">{{ stats.connectionsActive ?? 0 }}</div>
      </div>
      <div class="rounded-lg border px-2 py-2">
        <div class="text-muted-foreground">Получено</div>
        <div class="font-medium">{{ fmtBytes(stats.bytesDown) }}</div>
      </div>
      <div class="rounded-lg border px-2 py-2">
        <div class="text-muted-foreground">Последний путь</div>
        <div class="font-medium">{{ lastPath }}</div>
      </div>
    </div>

    <div
      v-if="t.error"
      class="mt-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
    >
      {{ t.error }}
    </div>
    <div
      v-else-if="!installed"
      class="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400"
    >
      Нужен sidecar tgws-proxy. Его можно скачать в расширенном режиме.
    </div>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      <Button
        v-if="running"
        size="sm"
        variant="outline"
        :loading="busy === 'copy'"
        :disabled="!!busy"
        @click="copyLink"
      >
        <Copy class="h-3.5 w-3.5" />
        Копировать ссылку
      </Button>
      <Button
        v-else-if="enabled && installed"
        size="sm"
        variant="outline"
        :loading="busy === 'start'"
        :disabled="!!busy"
        @click="start"
      >
        <Send class="h-3.5 w-3.5" />
        Запустить
      </Button>
      <Button size="sm" variant="ghost" @click="emit('advanced')">
        <Settings2 class="h-3.5 w-3.5" />
        Настройки
      </Button>
    </div>

    <p v-if="running" class="mt-2 text-xs text-muted-foreground">
      В Telegram: Настройки → Продвинутые → Тип соединения → Прокси → MTProto.
      Или нажмите «Открыть».
    </p>
  </div>
</template>
