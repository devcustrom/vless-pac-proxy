<script setup>
import { ref } from 'vue'
import { ListPlus, Plus, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'

const props = defineProps({ s: { type: Object, required: true } })

const domain = ref('')

async function add() {
  const value = domain.value.trim()
  if (!value) return
  try {
    await post('/api/extra', { action: 'add', domain: value })
    domain.value = ''
    notify('Домен добавлен')
  } catch {
    /* тост уже показан */
  }
}

async function remove(value) {
  try {
    await post('/api/extra', { action: 'remove', domain: value })
    notify('Домен убран')
  } catch {
    /* тост уже показан */
  }
}
</script>

<template>
  <Card title="Свои домены через VPN" description="Поддомены учитываются автоматически.">
    <template #icon>
      <ListPlus class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <form class="flex gap-2" @submit.prevent="add">
        <Input v-model="domain" placeholder="example.com" autocomplete="off" />
        <Button type="submit" variant="outline">
          <Plus class="h-3.5 w-3.5" />
          Добавить
        </Button>
      </form>

      <ul v-if="(s.extra || []).length" class="divide-y rounded-lg border">
        <li
          v-for="item in s.extra"
          :key="item"
          class="flex items-center justify-between px-3 py-2 text-sm"
        >
          <span class="font-mono text-xs">{{ item }}</span>
          <Button size="sm" variant="ghost" @click="remove(item)">
            <Trash2 class="h-3.5 w-3.5" />
          </Button>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Список пуст — домены также можно вписать в extra-domains.txt.
      </p>
    </div>
  </Card>
</template>
