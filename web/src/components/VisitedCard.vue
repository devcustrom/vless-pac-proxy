<script setup>
import { Eye, Plus } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'

const props = defineProps({ s: { type: Object, required: true } })

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
  <Card title="Недавние сайты" description="Открывались напрямую. Если сайту нужен VPN — добавьте.">
    <template #icon>
      <Eye class="h-4 w-4 text-primary" />
    </template>

    <ul v-if="(s.visited || []).length" class="divide-y rounded-lg border">
      <li
        v-for="item in (s.visited || []).slice(0, 30)"
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
  </Card>
</template>
