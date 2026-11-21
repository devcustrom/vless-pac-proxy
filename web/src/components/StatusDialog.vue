<script setup>
import { computed } from 'vue'
import Dialog from '@/components/ui/Dialog.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({
  open: { type: Boolean, default: false },
  s: { type: Object, required: true },
})
const emit = defineEmits(['close'])

const channels = computed(() => props.s.channels || {})
const systemProxy = computed(() => props.s.systemProxy || {})
const rows = computed(() => [
  {
    label: 'WireGuard',
    value: channels.value.wireguard ? 'включён' : 'выключен',
    ok: !!channels.value.wireguard,
  },
  {
    label: 'VLESS / Xray',
    value: channels.value.xray ? 'включён' : 'выключен',
    ok: !!channels.value.xray,
  },
  {
    label: 'Системный прокси',
    value: systemProxy.value.enabled ? systemProxy.value.value || 'включён' : 'выключен',
    ok: !!systemProxy.value.enabled,
  },
  {
    label: 'Умный прокси',
    value: props.s.smartProxyReady ? 'готов' : 'не слушает порт',
    ok: !!props.s.smartProxyReady,
  },
])
</script>

<template>
  <Dialog
    :open="open"
    title="Состояние подключения"
    description="Короткая сводка по каналам и локальным прокси."
    @close="emit('close')"
  >
    <div class="space-y-2">
      <div
        v-for="row in rows"
        :key="row.label"
        class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-2 text-sm"
      >
        <span>{{ row.label }}</span>
        <Badge :variant="row.ok ? 'success' : 'secondary'">{{ row.value }}</Badge>
      </div>
    </div>
    <template #footer>
      <Button variant="outline" @click="emit('close')">Закрыть</Button>
    </template>
  </Dialog>
</template>
