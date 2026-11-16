<script setup>
defineProps({
  modelValue: { type: String, required: true },
  tabs: { type: Array, default: () => [] },
})
const emit = defineEmits(['update:modelValue'])
</script>

<template>
  <div class="container pb-1">
    <div class="inline-flex flex-wrap items-center gap-1 rounded-lg bg-muted p-1 text-muted-foreground">
      <button
        v-for="tab in tabs"
        :key="tab.value"
        type="button"
        class="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors"
        :class="modelValue === tab.value
          ? 'bg-background text-foreground shadow-sm'
          : 'hover:text-foreground'"
        @click="emit('update:modelValue', tab.value)"
      >
        {{ tab.label }}
      </button>
    </div>
  </div>
</template>
