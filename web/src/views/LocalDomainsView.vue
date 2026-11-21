<script setup>
import LocalDomainsCard from '@/components/LocalDomainsCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <LocalDomainsCard :s="s" />
</template>
