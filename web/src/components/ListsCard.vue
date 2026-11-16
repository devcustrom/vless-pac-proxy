<script setup>
import { computed } from 'vue'
import { ListChecks, RefreshCw, RotateCcw } from 'lucide-vue-next'
import { fmtTime, notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({ s: { type: Object, required: true } })

const pacUrl = computed(() => `${window.location.origin}/proxy.pac`)

async function refreshLists() {
  try {
    await post('/api/refresh')
    notify('Обновление списков запущено')
  } catch {
    /* тост уже показан */
  }
}

async function restartProxy() {
  try {
    await post('/api/restart-proxy')
    notify('Движки перезапускаются')
  } catch {
    /* тост уже показан */
  }
}
</script>

<template>
  <Card title="Списки доменов" description="itdog + Re-filter RKN; обновление раз в 12 часов.">
    <template #icon>
      <ListChecks class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="grid grid-cols-3 gap-2 text-center">
        <div class="rounded-lg border bg-muted/30 px-2 py-3">
          <div class="text-lg font-semibold">{{ (s.proxied ?? 0).toLocaleString('ru-RU') }}</div>
          <div class="text-xs text-muted-foreground">через VPN</div>
        </div>
        <div class="rounded-lg border bg-muted/30 px-2 py-3">
          <div class="text-lg font-semibold">{{ (s.direct ?? 0).toLocaleString('ru-RU') }}</div>
          <div class="text-xs text-muted-foreground">напрямую</div>
        </div>
        <div class="rounded-lg border bg-muted/30 px-2 py-3">
          <div class="text-lg font-semibold">{{ (s.extra || []).length }}</div>
          <div class="text-xs text-muted-foreground">своих</div>
        </div>
      </div>

      <div class="text-xs text-muted-foreground">
        Обновлено: {{ fmtTime(s.lastUpdate) }}
        <span v-if="s.updateError" class="text-destructive"> · ошибка: {{ s.updateError }}</span>
      </div>

      <div class="flex flex-wrap gap-2">
        <Button size="sm" :disabled="s.refreshing" @click="refreshLists">
          <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': s.refreshing }" />
          Обновить списки
        </Button>
        <Button size="sm" variant="outline" @click="restartProxy">
          <RotateCcw class="h-3.5 w-3.5" />
          Перезапустить движки
        </Button>
        <a
          :href="pacUrl"
          target="_blank"
          class="inline-flex h-8 items-center rounded-md px-3 text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          proxy.pac
        </a>
      </div>
    </div>
  </Card>
</template>
