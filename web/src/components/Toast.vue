<script setup>
import { CheckCircle2, XCircle } from 'lucide-vue-next'
import { store } from '@/lib/store'
</script>

<template>
  <transition name="fade">
    <div
      v-if="store.toast"
      class="fixed bottom-4 right-4 z-50 flex max-w-sm items-start gap-2 rounded-xl border bg-card px-4 py-3 text-sm shadow-lg animate-toast-in"
      :class="store.toast.kind === 'error' ? 'border-destructive/40' : 'border-success/40'"
    >
      <XCircle v-if="store.toast.kind === 'error'" class="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
      <CheckCircle2 v-else class="mt-0.5 h-4 w-4 shrink-0 text-success" />
      <span>{{ store.toast.text }}</span>
    </div>
  </transition>
</template>
