<script setup>
import { cn } from '@/lib/utils'

const props = defineProps({
  modelValue: { type: Boolean, default: false },
  class: { type: String, default: '' },
})
const emit = defineEmits(['update:modelValue'])
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :class="cn(
      'peer inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50',
      modelValue ? 'bg-primary' : 'bg-input',
      props.class,
    )"
    @click="emit('update:modelValue', !modelValue)"
  >
    <span
      :class="cn(
        'pointer-events-none block h-4 w-4 rounded-full bg-background shadow-lg ring-0 transition-transform',
        modelValue ? 'translate-x-4' : 'translate-x-0',
      )"
    />
  </button>
</template>
