<script setup>
import { RouterLink } from 'vue-router'

defineProps({
  tabs: { type: Array, default: () => [] },
})
</script>

<template>
  <div class="container overflow-x-auto pb-1">
    <div class="inline-flex min-w-full flex-nowrap items-center gap-1 rounded-lg bg-muted p-1 text-muted-foreground">
      <RouterLink
        v-for="tab in tabs"
        :key="tab.to"
        :to="tab.to"
        custom
        v-slot="{ navigate }"
      >
        <button
          type="button"
          class="inline-flex shrink-0 items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
          :class="tab.active
            ? 'bg-background text-foreground shadow-sm'
            : 'hover:text-foreground'"
          @click="navigate"
        >
          {{ tab.label }}
        </button>
      </RouterLink>
    </div>
  </div>
</template>
