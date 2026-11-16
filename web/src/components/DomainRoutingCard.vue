<script setup>
import { ref } from 'vue'
import { ListFilter, Plus, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'

const props = defineProps({ s: { type: Object, required: true } })

const inputs = ref({ extra: '', wg: '', vless: '' })

const sections = [
  {
    key: 'extra',
    title: 'Свои домены через VPN',
    hint: 'Идут через основной канал (переключатель в «Обзоре»). Поддомены учитываются автоматически.',
    items: () => props.s.extra || [],
    add: async (domain) => post('/api/extra', { action: 'add', domain }),
    remove: async (domain) => post('/api/extra', { action: 'remove', domain }),
    badge: 'VPN',
    variant: 'secondary',
  },
  {
    key: 'wg',
    title: 'Только через WireGuard',
    hint: 'Даже если основной канал — VLESS. При мёртвом WireGuard уходят в VLESS.',
    items: () => props.s.wgDomains || [],
    add: async (domain) => post('/api/vless', { action: 'add-wg-domain', domain }),
    remove: async (domain) => post('/api/vless', { action: 'remove-wg-domain', domain }),
    badge: 'WG',
    variant: 'success',
  },
  {
    key: 'vless',
    title: 'Только через VLESS',
    hint: 'Даже если основной канал — WireGuard. При мёртвом VLESS уходят в WireGuard.',
    items: () => props.s.vlessDomains || [],
    add: async (domain) => post('/api/vless', { action: 'add-domain', domain }),
    remove: async (domain) => post('/api/vless', { action: 'remove-domain', domain }),
    badge: 'VLESS',
    variant: 'warning',
  },
]

async function add(section) {
  const value = inputs.value[section.key].trim()
  if (!value) return
  try {
    await section.add(value)
    inputs.value[section.key] = ''
    notify('Домен добавлен: ' + value)
  } catch {
    /* тост показан */
  }
}

async function remove(section, value) {
  try {
    await section.remove(value)
    notify('Домен убран')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Маршрутизация доменов"
    description="Приоритет: исключения → «только VLESS» → «только WireGuard» → общий VPN-список."
  >
    <template #icon>
      <ListFilter class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-5">
      <section v-for="section in sections" :key="section.key" class="space-y-2">
        <div class="flex flex-wrap items-center gap-2">
          <Badge :variant="section.variant">{{ section.badge }}</Badge>
          <h4 class="text-sm font-medium">{{ section.title }}</h4>
          <span class="text-xs text-muted-foreground">{{ section.items().length }}</span>
        </div>
        <p class="text-xs text-muted-foreground">{{ section.hint }}</p>

        <ul v-if="section.items().length" class="divide-y rounded-lg border">
          <li
            v-for="item in section.items()"
            :key="item"
            class="flex items-center justify-between px-3 py-1.5 text-sm"
          >
            <span class="font-mono text-xs">{{ item }}</span>
            <Button size="sm" variant="ghost" @click="remove(section, item)">
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </li>
        </ul>

        <form class="flex gap-2" @submit.prevent="add(section)">
          <Input
            v-model="inputs[section.key]"
            placeholder="example.com"
            autocomplete="off"
          />
          <Button type="submit" variant="outline" size="sm">
            <Plus class="h-3.5 w-3.5" />
            Добавить
          </Button>
        </form>
      </section>
    </div>
  </Card>
</template>
