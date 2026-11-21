<script setup>
import DomainRoutingCard from '@/components/DomainRoutingCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <DomainRoutingCard :s="s" />
</template>
