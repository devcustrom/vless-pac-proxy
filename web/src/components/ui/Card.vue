<script setup>
defineProps({
  title: { type: String, default: '' },
  description: { type: String, default: '' },
})
</script>

<template>
  <div class="rounded-xl border bg-card text-card-foreground shadow-sm">
    <div v-if="title || $slots.header" class="flex flex-col space-y-1.5 p-6 pb-3">
      <slot name="header">
        <h3 class="flex items-center gap-2 text-base font-semibold leading-none tracking-tight">
          <slot name="icon" />
          {{ title }}
        </h3>
        <p v-if="description" class="text-sm text-muted-foreground">{{ description }}</p>
      </slot>
    </div>
    <div class="p-6" :class="{ 'pt-0': title || $slots.header }">
      <slot />
    </div>
    <div v-if="$slots.footer" class="flex items-center p-6 pt-0">
      <slot name="footer" />
    </div>
  </div>
</template>
