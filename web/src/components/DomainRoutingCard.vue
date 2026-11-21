<script setup>
import { computed, ref } from 'vue'
import { ListFilter, Plus, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Dialog from '@/components/ui/Dialog.vue'
import DomainAddForm from '@/components/DomainAddForm.vue'
import Select from '@/components/ui/Select.vue'

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

const addOpen = ref(false)
const target = ref('extra')
const targetOptions = sections.map((section) => ({
  value: section.key,
  label: section.title,
}))
const selectedSection = computed(
  () => sections.find((section) => section.key === target.value) || sections[0],
)

async function submitAdd(output) {
  const ok = await add(selectedSection.value, output.domain)
  if (ok) addOpen.value = false
  return ok
}

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

    <div class="space-y-3">
      <div class="flex justify-end">
        <Button size="sm" @click="addOpen = true">
          <Plus class="h-3.5 w-3.5" />
          Добавить домен
        </Button>
      </div>

      <details
        v-for="section in sections"
        :key="section.key"
        class="rounded-lg border bg-muted/30 px-3 py-2"
      >
        <summary class="flex cursor-pointer flex-wrap items-center gap-2 text-sm">
          <Badge :variant="section.variant">{{ section.badge }}</Badge>
          <span class="font-medium">{{ section.title }}</span>
          <span class="text-xs text-muted-foreground">{{ section.items().length }}</span>
        </summary>
        <div class="mt-3 space-y-2">
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
                :aria-label="`Удалить ${item}`"
                :title="`Удалить ${item}`"
                @click="remove(section, item)"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </Button>
            </li>
          </ul>
          <p v-else class="text-xs text-muted-foreground">Пока пусто.</p>
        </div>
      </details>
    </div>

    <Dialog
      :open="addOpen"
      title="Добавить домен"
      description="Выберите, как маршрутизировать домен."
      @close="addOpen = false"
    >
      <div class="space-y-3">
        <div class="space-y-1">
          <div class="text-xs font-medium text-muted-foreground">Маршрут</div>
          <Select v-model="target" :options="targetOptions" />
        </div>
        <DomainAddForm
          :submit="submitAdd"
          :loading="addLoading"
          :disabled="!!removing"
        />
      </div>
    </Dialog>
  </Card>
</template>
