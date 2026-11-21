<script setup>
import TelegramCard from '@/components/TelegramCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="grid items-start gap-4 lg:grid-cols-2">
    <div class="space-y-4">
      <TelegramCard :s="s" />
    </div>
  </div>
</template>
