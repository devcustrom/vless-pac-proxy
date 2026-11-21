<script setup>
import { ArrowLeft } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import ZaprettCatalogCard from '@/components/ZaprettCatalogCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="space-y-4">
    <RouterLink
      to="/dpi"
      class="inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground"
    >
      <ArrowLeft class="h-4 w-4" />
      Назад к обходу DPI
    </RouterLink>
    <ZaprettCatalogCard :s="s" />
  </div>
</template>
