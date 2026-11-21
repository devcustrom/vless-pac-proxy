<script setup>
import OnboardingCard from '@/components/OnboardingCard.vue'
import ChannelsCard from '@/components/ChannelsCard.vue'
import ListsCard from '@/components/ListsCard.vue'
import QuickCheckCard from '@/components/QuickCheckCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="grid items-start gap-4 lg:grid-cols-2">
    <div class="space-y-4">
      <OnboardingCard :s="s" />
      <ChannelsCard :s="s" />
    </div>
    <div class="space-y-4">
      <ListsCard :s="s" />
      <QuickCheckCard :s="s" />
    </div>
  </div>
</template>
