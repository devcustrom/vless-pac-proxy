<script setup>
import { Check } from 'lucide-vue-next'
import { CheckboxIndicator, CheckboxRoot } from 'reka-ui'
import { cn } from '@/lib/utils'

const props = defineProps({
  modelValue: { type: Boolean, default: false },
  disabled: Boolean,
  class: { type: String, default: '' },
})
const emit = defineEmits(['update:modelValue'])
</script>

<template>
  <CheckboxRoot
    :model-value="modelValue"
    :disabled="disabled"
    :class="
      cn(
        'peer h-4 w-4 shrink-0 rounded-sm border border-primary shadow focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground',
        props.class,
      )
    "
    @update:model-value="emit('update:modelValue', $event)"
  >
    <CheckboxIndicator class="flex items-center justify-center text-current">
      <Check class="h-3 w-3" />
    </CheckboxIndicator>
  </CheckboxRoot>
</template>
