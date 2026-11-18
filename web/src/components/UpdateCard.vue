<script setup>
import { computed, ref } from 'vue'
import { Download, RefreshCw, Sparkles } from 'lucide-vue-next'
import { api, notify, store } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const info = ref(null)
const busy = ref(false)
const installing = ref(false)

const current = computed(() => store.status?.version || '—')

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

async function check() {
  busy.value = true
  try {
    info.value = await api('/api/update', undefined, 'GET')
    if (info.value.available) {
      notify(`Доступно обновление v${info.value.latest}`)
    } else {
      notify('Установлена последняя версия')
    }
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = false
  }
}

async function install() {
  if (!info.value?.available || installing.value) return
  installing.value = true
  try {
    await api('/api/update/install', {})
    notify('Обновление скачано, менеджер перезапускается…')
    // Ждём, пока старый процесс выйдет, updater заменит файл и поднимется
    // новый менеджер, затем перезагружаем вкладку уже на новой версии.
    for (let i = 0; i < 45; i += 1) {
      await sleep(1000)
      try {
        const response = await fetch('/api/status')
        if (!response.ok) continue
        const status = await response.json()
        if (status.version && status.version !== current.value) {
          window.location.reload()
          return
        }
      } catch {
        /* менеджер перезапускается */
      }
    }
    window.location.reload()
  } catch (error) {
    installing.value = false
    notify(error.message, 'error')
  }
}
</script>

<template>
  <Card title="Обновление" description="Последний релиз и прямая ссылка на бинарник.">
    <template #icon>
      <Sparkles class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex flex-wrap items-center gap-2 text-sm">
        <span class="text-muted-foreground">Установлено:</span>
        <Badge variant="outline">v{{ current }}</Badge>
        <template v-if="info">
          <span class="text-muted-foreground">Последний:</span>
          <Badge :variant="info.available ? 'warning' : 'success'">v{{ info.latest }}</Badge>
        </template>
      </div>

      <p v-if="info" class="text-xs text-muted-foreground">
        <template v-if="info.available">
          Новая версия {{ info.asset || '' }}. Нажмите «Установить» — приложение
          скачает бинарник, заменит его и перезапустит менеджер. Системный прокси
          останется включённым, данные и настройки сохранятся.
        </template>
        <template v-else>
          Обновлений нет — установлена актуальная версия.
        </template>
      </p>

      <div class="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" :disabled="busy || installing" @click="check">
          <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': busy }" />
          Проверить обновления
        </Button>
        <Button
          v-if="info?.available"
          size="sm"
          :loading="installing"
          @click="install"
        >
          <Download class="h-3.5 w-3.5" />
          Установить v{{ info.latest }}
        </Button>
        <a
          v-if="info?.available && (info.url || info.page)"
          :href="info.url || info.page"
          target="_blank"
          rel="noopener"
          class="inline-flex h-8 items-center gap-2 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
        >
          Скачать вручную
        </a>
      </div>
    </div>
  </Card>
</template>
