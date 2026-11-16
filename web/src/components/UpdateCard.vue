<script setup>
import { computed, ref } from 'vue'
import { Download, RefreshCw, Sparkles } from 'lucide-vue-next'
import { api, notify, store } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const info = ref(null)
const busy = ref(false)

const current = computed(() => store.status?.version || '—')

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
          Новая версия {{ info.asset || '' }}. Скачайте бинарник и замените им файл при
          остановленном менеджере (Windows) или положите рядом и перезапустите (macOS).
        </template>
        <template v-else>
          Обновлений нет — установлена актуальная версия.
        </template>
      </p>

      <div class="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" :disabled="busy" @click="check">
          <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': busy }" />
          Проверить обновления
        </Button>
        <a
          v-if="info?.available && (info.url || info.page)"
          :href="info.url || info.page"
          target="_blank"
          class="inline-flex h-8 items-center gap-2 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90"
        >
          <Download class="h-3.5 w-3.5" />
          Скачать v{{ info.latest }}
        </a>
      </div>
    </div>
  </Card>
</template>
