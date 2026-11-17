<script setup>
import Dialog from '@/components/ui/Dialog.vue'
import DomainRoutingCard from '@/components/DomainRoutingCard.vue'

defineProps({
  open: Boolean,
  s: { type: Object, required: true },
})
const emit = defineEmits(['close'])
</script>

<template>
  <Dialog
    :open="open"
    wide
    title="Маршрутизация доменов"
    description="Свои домены, только WireGuard и только VLESS. Изменения применяются сразу."
    @close="emit('close')"
  >
    <DomainRoutingCard :s="s" />
  </Dialog>
</template>
