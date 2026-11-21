<script setup>
import { computed } from 'vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
import { cn } from '@/lib/utils'

defineProps({ s: { type: Object, required: true } })

const route = useRoute()
const tabs = [
  { label: 'Локальные', to: '/domains/local' },
  { label: 'Маршрутизация', to: '/domains/routing' },
]
const active = computed(() => route.path)
</script>

<template>
  <div class="space-y-4">
    <div class="inline-flex flex-wrap items-center gap-1 rounded-lg bg-muted p-1 text-sm text-muted-foreground">
      <RouterLink
        v-for="item in tabs"
        :key="item.to"
        :to="item.to"
        custom
        v-slot="{ navigate }"
      >
        <button
          type="button"
          :class="
            cn(
              'rounded-md px-3 py-1.5 font-medium transition-colors',
              active === item.to
                ? 'bg-background text-foreground shadow-sm'
                : 'hover:text-foreground',
            )
          "
          @click="navigate"
        >
          {{ item.label }}
        </button>
      </RouterLink>
    </div>
    <RouterView v-slot="{ Component }">
      <component :is="Component" :s="s" />
    </RouterView>
  </div>
</template>
