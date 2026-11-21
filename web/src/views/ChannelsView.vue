<script setup>
import WireGuardCard from '@/components/WireGuardCard.vue'
import VlessCard from '@/components/VlessCard.vue'
import PoolCard from '@/components/PoolCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="grid items-start gap-4 lg:grid-cols-2">
    <div class="space-y-4">
      <WireGuardCard :s="s" />
      <VlessCard :s="s" />
    </div>
    <div class="space-y-4">
      <PoolCard :s="s" />
    </div>
  </div>
</template>
