<script setup>
import { computed } from 'vue'
import { Info, Settings2 } from 'lucide-vue-next'
import { useRouter } from 'vue-router'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({
  s: { type: Object, required: true },
  plugin: { type: String, default: '' },
})

const router = useRouter()

const META = {
  telegram: {
    title: 'Telegram-прокси',
    description: 'Локальный MTProto-прокси для Telegram Desktop с обходом через WS/VPN.',
  },
  zapret: {
    title: 'Обход DPI',
    description: 'Zapret 2, стратегии, списки и каталог Zaprett.',
  },
  localDomains: {
    title: 'Локальные домены',
    description: 'HTTPS-домены с локальным CA, hosts и обратным прокси.',
  },
}

const meta = computed(() => META[props.plugin] || { title: 'Плагин', description: '' })
const supported = computed(() => props.plugin !== 'zapret' || props.s?.zapret?.supported !== false)
</script>

<template>
  <Card :title="meta.title" :description="meta.description">
    <template #icon>
      <Info class="h-4 w-4 text-primary" />
    </template>
    <div class="flex flex-col items-start gap-3 rounded-lg border bg-muted/30 px-4 py-4">
      <Badge variant="secondary">плагин выключен</Badge>
      <p v-if="supported" class="text-sm text-muted-foreground">
        Включите плагин в настройках — после этого раздел станет доступен.
      </p>
      <p v-else class="text-sm text-muted-foreground">
        Этот плагин доступен только на Windows.
      </p>
      <Button v-if="supported" size="sm" @click="router.push('/settings')">
        <Settings2 class="h-3.5 w-3.5" />
        Включить в Настройках
      </Button>
    </div>
  </Card>
</template>
