<script setup>
import ZapretSimpleCard from '@/components/ZapretSimpleCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="mx-auto max-w-3xl">
    <ZapretSimpleCard :s="s" />
  </div>
</template>
