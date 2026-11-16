<script setup>
import { ref } from 'vue'
import { FileUp, Save, Shield, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Label from '@/components/ui/Label.vue'
import Textarea from '@/components/ui/Textarea.vue'

const props = defineProps({ s: { type: Object, required: true } })

const content = ref('')
const busy = ref(false)

async function pickFile(event) {
  const file = event.target.files?.[0]
  if (!file) return
  content.value = await file.text()
  event.target.value = ''
  notify('Файл загружен — нажмите «Сохранить wg.conf»')
}

async function save() {
  if (!content.value.trim()) {
    notify('Вставьте содержимое wg.conf', 'error')
    return
  }
  busy.value = true
  try {
    const result = await post('/api/wg', { action: 'set', content: content.value })
    content.value = ''
    notify('wg.conf сохранён' + (result.endpoint ? `: ${result.endpoint}` : ''))
  } catch {
    /* тост уже показан */
  } finally {
    busy.value = false
  }
}

async function remove() {
  if (!confirm('Удалить wg.conf и остановить WireGuard?')) return
  try {
    await post('/api/wg', { action: 'clear' })
    notify('wg.conf удалён')
  } catch {
    /* тост уже показан */
  }
}
</script>

<template>
  <Card
    title="WireGuard — конфиг"
    description="Вставьте wg.conf или выберите файл. Конфиг хранится локально; в панели ключи не показываются."
  >
    <template #icon>
      <Shield class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="s.wgConfigured ? 'success' : 'secondary'">
          {{ s.wgConfigured ? 'настроен' : 'не настроен' }}
        </Badge>
        <span class="text-muted-foreground">Endpoint: {{ s.wgEndpoint || '—' }}</span>
      </div>

      <div class="space-y-1.5">
        <Label for="wg-text">Новый конфиг</Label>
        <Textarea
          id="wg-text"
          v-model="content"
          rows="7"
          spellcheck="false"
          class="font-mono text-xs"
          placeholder="[Interface]&#10;PrivateKey = ...&#10;Address = 10.66.66.2/32&#10;&#10;[Peer]&#10;PublicKey = ...&#10;Endpoint = host:51820"
        />
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <label
          class="inline-flex h-9 cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-3 text-sm hover:bg-accent"
        >
          <FileUp class="h-4 w-4" />
          Выбрать файл
          <input type="file" accept=".conf,.txt,text/plain" class="hidden" @change="pickFile" />
        </label>
        <Button size="sm" :disabled="busy || !content.trim()" @click="save">
          <Save class="h-3.5 w-3.5" />
          Сохранить wg.conf
        </Button>
        <Button
          size="sm"
          variant="ghost"
          class="text-destructive hover:text-destructive"
          @click="remove"
        >
          <Trash2 class="h-3.5 w-3.5" />
          Удалить
        </Button>
      </div>
    </div>
  </Card>
</template>
