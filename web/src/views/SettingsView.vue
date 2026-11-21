<script setup>
import SettingsCard from '@/components/SettingsCard.vue'
import UpdateCard from '@/components/UpdateCard.vue'
import BackupCard from '@/components/BackupCard.vue'

defineProps({ s: { type: Object, required: true } })
</script>

<template>
  <div class="grid items-start gap-4 lg:grid-cols-2">
    <div class="space-y-4">
      <SettingsCard :s="s" />
    </div>
    <div class="space-y-4">
      <UpdateCard />
      <BackupCard />
    </div>
  </div>
</template>
