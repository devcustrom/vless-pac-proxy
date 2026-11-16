<script setup>
import { computed, ref } from 'vue'
import { Blocks, Info, Settings2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const busy = ref(false)
const localDomains = ref(!!props.s.plugins?.localDomains)

const core = computed(() => [
  {
    name: 'Умный прокси',
    description: 'Маршрутизация по доменам на 127.0.0.1:8081, списки и PAC.',
  },
  {
    name: 'Списки доменов',
    description: 'itdog + Re-filter RKN, обновление по расписанию.',
  },
  {
    name: 'Пул конфигов',
    description: 'Подписки, live-проверка и автопереключение Xray.',
  },
  {
    name: 'Каналы',
    description: 'WireGuard и Xray; включаются отдельно в карточке каналов.',
  },
])

async function toggleLocalDomains(value) {
  busy.value = true
  localDomains.value = value
  try {
    await post('/api/plugins', { localDomains: value })
    notify(value ? 'Плагин локальных доменов включён' : 'Плагин локальных доменов выключен')
  } catch {
    localDomains.value = !value
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Card
    title="Плагины"
    description="По умолчанию плагины выключены. Включайте только нужные."
  >
    <template #icon>
      <Blocks class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">Локальные домены</span>
            <Badge :variant="localDomains ? 'success' : 'secondary'">
              {{ localDomains ? 'включён' : 'выключен' }}
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            HTTP :{{ s.localProxy?.port }} / HTTPS :{{ s.localProxy?.httpsPort }}, свой CA,
            WebSocket. Слушатели работают только когда плагин включён.
          </p>
        </div>
        <Switch
          :model-value="localDomains"
          :disabled="busy"
          @update:model-value="toggleLocalDomains"
        />
      </div>

      <div class="space-y-2">
        <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <Info class="h-3.5 w-3.5" />
          Ядро (всегда включено)
        </div>
        <div
          v-for="item in core"
          :key="item.name"
          class="rounded-lg border px-3 py-2"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm">{{ item.name }}</span>
            <Badge variant="outline">ядро</Badge>
          </div>
          <p class="mt-0.5 text-xs text-muted-foreground">{{ item.description }}</p>
        </div>
      </div>

      <p class="text-xs text-muted-foreground">
        Как добавить свой плагин или протокол — «Документация» → «Плагины и протоколы».
      </p>
    </div>
  </Card>
</template>
