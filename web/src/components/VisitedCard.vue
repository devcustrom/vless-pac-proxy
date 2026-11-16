<script setup>
import { computed, ref, watch } from 'vue'
import { ChevronLeft, ChevronRight, Eye, Plus } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({ s: { type: Object, required: true } })

const PAGE_SIZE = 20
const page = ref(1)

const items = computed(() => {
  const cutoff = Math.floor(Date.now() / 1000) - 86_400
  return (props.s.visited || []).filter((item) => item.last >= cutoff)
})
const total = computed(() => props.s.visitedTotal ?? items.value.length)
const pageCount = computed(() => Math.max(1, Math.ceil(items.value.length / PAGE_SIZE)))
const visible = computed(() => {
  const start = (page.value - 1) * PAGE_SIZE
  return items.value.slice(start, start + PAGE_SIZE)
})

watch(pageCount, (count) => {
  if (page.value > count) page.value = count
})

async function addToVpn(domain) {
  try {
    await post('/api/extra', { action: 'add', domain })
    notify(domain + ' добавлен в VPN-список')
  } catch {
    /* тост уже показан */
  }
}
</script>

<template>
  <Card
    title="Недавние сайты"
    description="Уникальные домены за последние 24 часа. Если сайту нужен VPN — добавьте."
  >
    <template #icon>
      <Eye class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <ul v-if="visible.length" class="divide-y rounded-lg border">
        <li
          v-for="item in visible"
          :key="item.domain"
          class="flex items-center gap-2 px-3 py-2 text-sm"
        >
          <span class="min-w-0 flex-1 truncate">
            {{ item.domain }}
            <span class="text-xs text-muted-foreground">×{{ item.count }}</span>
          </span>
          <Button size="sm" variant="outline" @click="addToVpn(item.domain)">
            <Plus class="h-3.5 w-3.5" />
            через VPN
          </Button>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Пока пусто — список наполнится, когда включён системный прокси.
      </p>

      <div
        v-if="items.length > PAGE_SIZE"
        class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground"
      >
        <span>
          всего {{ total }} · страница {{ page }} из {{ pageCount }}
        </span>
        <div class="flex gap-1">
          <Button
            size="sm"
            variant="outline"
            :disabled="page <= 1"
            @click="page--"
          >
            <ChevronLeft class="h-3.5 w-3.5" />
            Назад
          </Button>
          <Button
            size="sm"
            variant="outline"
            :disabled="page >= pageCount"
            @click="page++"
          >
            Вперёд
            <ChevronRight class="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    </div>
  </Card>
</template>
