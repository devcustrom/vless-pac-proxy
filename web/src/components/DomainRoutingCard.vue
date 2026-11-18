<script setup>
import { computed, ref } from 'vue'
import { ListFilter, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import DomainAddForm from '@/components/DomainAddForm.vue'

const props = defineProps({ s: { type: Object, required: true } })

const addAction = useActionMutation(
  ({ section, domain }) => section.add(domain),
  [['status']],
)
const addLoading = computed(() => addAction.isLoading.value)
const addSection = ref('')
const removeAction = useActionMutation(
  ({ section, value }) => section.remove(value),
  [['status']],
)
const removing = ref('')

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

async function add(section, domain) {
  addSection.value = section.key
  try {
    await addAction.mutateAsync({ section, domain })
    notify('Домен добавлен: ' + domain)
    return true
  } catch {
    /* тост показан */
    return false
  } finally {
    addSection.value = ''
  }
}

async function remove(section, value) {
  if (!confirm(`Убрать ${value} из «${section.title}»?`)) return
  const key = `${section.key}:${value}`
  removing.value = key
  try {
    await removeAction.mutateAsync({ section, value })
    notify('Домен убран')
  } catch {
    /* тост показан */
  } finally {
    removing.value = ''
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
            <Button
              size="sm"
              variant="ghost"
              :loading="removing === `${section.key}:${item}`"
              :disabled="!!removing || addLoading"
              @click="remove(section, item)"
            >
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </li>
        </ul>

        <DomainAddForm
          :submit="(output) => add(section, output.domain)"
          :loading="addSection === section.key"
          :disabled="!!removing || (!!addSection && addSection !== section.key)"
        />
      </section>
    </div>
  </Card>
</template>
