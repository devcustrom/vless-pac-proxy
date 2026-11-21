<script setup>
import ZapretCard from '@/components/ZapretCard.vue'
import CatalogSummaryCard from '@/components/CatalogSummaryCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="grid items-start gap-4 lg:grid-cols-2">
    <div class="space-y-4">
      <ZapretCard :s="s" />
    </div>
    <div class="space-y-4">
      <CatalogSummaryCard :s="s" />
    </div>
  </div>
</template>
