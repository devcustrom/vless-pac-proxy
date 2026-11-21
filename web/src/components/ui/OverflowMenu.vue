<script setup>
import { MoreHorizontal } from 'lucide-vue-next'

defineProps({
  label: { type: String, default: 'Действия' },
})
</script>

<template>
  <details class="relative">
    <summary
      class="flex h-8 w-8 cursor-pointer list-none items-center justify-center rounded-md border bg-background text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground [&::-webkit-details-marker]:hidden"
      :aria-label="label"
      :title="label"
    >
      <MoreHorizontal class="h-4 w-4" />
    </summary>
    <div
      class="absolute right-0 z-20 mt-1 min-w-44 rounded-md border bg-card p-1 text-card-foreground shadow-lg"
      @click="$el.closest('details')?.removeAttribute('open')"
    >
      <slot />
    </div>
  </details>
</template>
