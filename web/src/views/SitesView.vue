<script setup>
import VisitedCard from '@/components/VisitedCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="space-y-4">
    <VisitedCard :s="s" />
  </div>
</template>
