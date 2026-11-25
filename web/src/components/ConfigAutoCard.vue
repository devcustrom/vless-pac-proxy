<script setup>
import { computed, ref, watch } from 'vue'
import { Gauge, RefreshCw } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const enabled = ref(props.s.autoRotate !== false)
const busy = ref(false)
const running = computed(() => !!props.s.autoConfigRunning)

watch(
  () => props.s.autoRotate,
  (value) => {
    if (!busy.value) enabled.value = value !== false
  },
)

async function toggle(value) {
  enabled.value = value
  busy.value = true
  try {
    await post('/api/auto', { autoRotate: value })
    notify(value ? 'Автовыбор конфигов включён' : 'Автовыбор конфигов выключен')
  } catch {
    enabled.value = !value
  } finally {
    busy.value = false
  }
}

async function runNow() {
  try {
    await post('/api/configs/auto', {})
    notify('Проверяю все конфиги — выберу самый быстрый')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Автовыбор конфигураций"
    description="Тестируем все сохранённые WireGuard и Xray/Outline-конфиги, а также пул подписок, и выбираем самый быстрый рабочий."
  >
    <template #icon>
      <Gauge class="h-4 w-4 text-primary" />
    </template>

    <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-2">
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium">Выбирать самый быстрый автоматически</span>
          <Badge v-if="enabled" variant="success">вкл</Badge>
          <Badge v-else variant="secondary">выкл</Badge>
        </div>
        <p class="mt-1 text-xs text-muted-foreground">
          Проверка идёт при запуске и раз в 15 минут. Когда режим включён, ручной
          выбор конфига может быть заменён на более быстрый.
        </p>
      </div>
      <Switch
        data-testid="auto-configs"
        aria-label="Выбирать самый быстрый конфиг автоматически"
        :model-value="enabled"
        :disabled="busy || running"
        @update:model-value="toggle"
      />
    </div>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      <Button
        size="sm"
        variant="outline"
        :loading="running"
        :disabled="running"
        @click="runNow"
      >
        <RefreshCw class="h-3.5 w-3.5" />
        Проверить и выбрать сейчас
      </Button>
      <span v-if="running" class="text-xs text-muted-foreground">
        Проверяю конфиги, это может занять несколько минут…
      </span>
    </div>
  </Card>
</template>
