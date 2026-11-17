<script setup>
import { onBeforeUnmount, onMounted } from 'vue'
import { X } from 'lucide-vue-next'
import Button from './Button.vue'

const props = defineProps({
  open: { type: Boolean, default: false },
  title: { type: String, default: '' },
  description: { type: String, default: '' },
  wide: Boolean,
})
const emit = defineEmits(['close'])

function onKey(event) {
  if (event.key === 'Escape' && props.open) emit('close')
}

onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-3 sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[90vh] w-full flex-col overflow-hidden rounded-2xl border bg-card shadow-2xl"
        :class="wide ? 'max-w-4xl' : 'max-w-lg'"
      >
        <div class="flex items-start gap-3 border-b px-5 py-3">
          <div class="min-w-0 flex-1">
            <h2 class="text-base font-semibold">{{ title }}</h2>
            <p v-if="description" class="mt-0.5 text-xs text-muted-foreground">{{ description }}</p>
          </div>
          <Button variant="ghost" size="icon" @click="emit('close')">
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="min-h-0 flex-1 overflow-auto px-5 py-4">
          <slot />
        </div>
        <div v-if="$slots.footer" class="flex flex-wrap justify-end gap-2 border-t px-5 py-3">
          <slot name="footer" />
        </div>
      </div>
    </div>
  </teleport>
</template>
