<script setup>
import { computed } from 'vue'
import { ExternalLink, Layers } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'

const props = defineProps({ s: { type: Object, required: true } })
const cat = computed(() => props.s.zapret?.catalog || {})
const active = computed(() => (cat.value.strategies || []).find((item) => item.active) || null)
const downloaded = computed(
  () => (cat.value.strategies || []).filter((item) => item.downloaded).length,
)
const source = computed(() => cat.value.sourcePage || cat.value.source || '')
</script>

<template>
  <Card
    title="Каталог Zaprett"
    description="Готовые стратегии, списки и payload'ы из внешнего репозитория."
  >
    <template #icon>
      <Layers class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3 text-sm">
      <div class="flex flex-wrap items-center gap-2">
        <Badge :variant="cat.cached ? 'success' : 'warning'">
          {{ cat.cached ? 'каталог загружен' : 'каталог не загружен' }}
        </Badge>
        <Badge variant="outline">скачано стратегий: {{ downloaded }}</Badge>
        <Badge v-if="active" variant="success">активна: {{ active.name || active.id }}</Badge>
      </div>
      <p v-if="active" class="text-xs text-muted-foreground">
        Автор: {{ active.author || 'не указан' }}
        <a
          v-if="active.source"
          :href="active.source"
          target="_blank"
          rel="noreferrer"
          class="ml-1 inline-flex items-center gap-1 text-primary hover:underline"
        >
          <ExternalLink class="h-3 w-3" />
          источник
        </a>
      </p>
      <p v-else class="text-xs text-muted-foreground">
        Откройте каталог, загрузите манифесты и выберите стратегию.
      </p>
      <a
        v-if="source"
        :href="source"
        target="_blank"
        rel="noreferrer"
        class="block break-all text-xs text-primary hover:underline"
      >
        <ExternalLink class="mr-1 inline h-3 w-3" />
        {{ source }}
      </a>
      <RouterLink
        to="/dpi/catalog"
        class="inline-flex h-8 items-center rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground shadow hover:bg-primary/90"
      >
        Открыть каталог
      </RouterLink>
    </div>
  </Card>
</template>
