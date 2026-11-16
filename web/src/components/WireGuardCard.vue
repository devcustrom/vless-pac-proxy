<script setup>
import { computed, onMounted, reactive, ref } from 'vue'
import { CheckCircle2, FileUp, FlaskConical, Plus, Shield, Trash2, Zap } from 'lucide-vue-next'
import { api, notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import Textarea from '@/components/ui/Textarea.vue'

const props = defineProps({ s: { type: Object, required: true } })

const configs = ref([])
const name = ref('')
const content = ref('')
const busy = ref(false)
const tests = reactive({})

const active = computed(() => configs.value.find((item) => item.active))

async function load() {
  try {
    const data = await api('/api/wg/configs', undefined, 'GET')
    configs.value = data.configs || []
  } catch (error) {
    notify(error.message, 'error')
  }
}

onMounted(load)

async function addConfig() {
  if (!name.value.trim() || !content.value.trim()) {
    notify('Укажите имя и вставьте wg.conf', 'error')
    return
  }
  busy.value = true
  try {
    await post('/api/wg/configs', {
      action: 'add',
      name: name.value.trim(),
      content: content.value,
    })
    name.value = ''
    content.value = ''
    await load()
    notify('WireGuard-конфиг добавлен')
  } catch {
    /* тост показан */
  } finally {
    busy.value = false
  }
}

async function pickFile(event) {
  const file = event.target.files?.[0]
  if (!file) return
  content.value = await file.text()
  if (!name.value.trim()) {
    name.value = file.name.replace(/\.(conf|txt)$/i, '')
  }
  event.target.value = ''
  notify('Файл загружен — нажмите «Добавить»')
}

async function useConfig(item) {
  try {
    await post('/api/wg/configs', { action: 'use', slug: item.slug })
    await load()
    notify('Активный WireGuard: ' + item.name)
  } catch {
    /* тост показан */
  }
}

async function removeConfig(item) {
  if (!confirm(`Удалить конфиг «${item.name}»?`)) return
  try {
    await post('/api/wg/configs', { action: 'remove', slug: item.slug })
    delete tests[item.slug]
    await load()
    notify('Конфиг удалён')
  } catch {
    /* тост показан */
  }
}

async function testConfig(item) {
  tests[item.slug] = { busy: true }
  try {
    const result = await api('/api/wg/configs', { action: 'test', slug: item.slug })
    tests[item.slug] = { ok: true, ms: result.ms, ip: result.ip }
  } catch (error) {
    tests[item.slug] = { ok: false, error: error.message }
  }
}
</script>

<template>
  <Card
    title="WireGuard — конфиги"
    description="Можно держать несколько конфигов: переключаться и проверять трафик. Активный копируется в wg.conf."
  >
    <template #icon>
      <Shield class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="s.wgConfigured ? 'success' : 'secondary'">
          {{ s.wgConfigured ? 'настроен' : 'не настроен' }}
        </Badge>
        <span class="text-muted-foreground">
          активный: {{ active?.name || '—' }} · {{ s.wgEndpoint || '—' }}
        </span>
      </div>

      <ul v-if="configs.length" class="max-h-64 divide-y overflow-auto rounded-lg border scrollbar-thin">
        <li v-for="item in configs" :key="item.slug" class="px-3 py-2">
          <div class="flex flex-wrap items-center gap-2 text-sm">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium">{{ item.name }}</span>
                <Badge v-if="item.active" variant="success">активен</Badge>
                <span class="truncate text-xs text-muted-foreground">{{ item.endpoint || '—' }}</span>
              </div>
              <div v-if="tests[item.slug]" class="mt-1 text-xs">
                <span v-if="tests[item.slug].busy" class="text-muted-foreground">проверяю трафик…</span>
                <Badge v-else-if="tests[item.slug].ok" variant="success">
                  <CheckCircle2 class="h-3 w-3" />
                  {{ tests[item.slug].ip }} · {{ tests[item.slug].ms }} мс
                </Badge>
                <Badge v-else variant="destructive">{{ tests[item.slug].error }}</Badge>
              </div>
            </div>
            <Button
              v-if="!item.active"
              size="sm"
              variant="outline"
              @click="useConfig(item)"
            >
              <Zap class="h-3.5 w-3.5" />
              Использовать
            </Button>
            <Button
              size="sm"
              variant="outline"
              :disabled="tests[item.slug]?.busy"
              @click="testConfig(item)"
            >
              <FlaskConical class="h-3.5 w-3.5" />
              Тест
            </Button>
            <Button size="sm" variant="ghost" @click="removeConfig(item)">
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Конфигов нет. Добавьте первый — он станет активным.
      </p>

      <div class="space-y-2 rounded-lg border bg-muted/30 p-3">
        <div class="flex items-center gap-2 text-sm font-medium">
          <Plus class="h-4 w-4" />
          Добавить конфиг
        </div>
        <div class="space-y-1">
          <Label>Имя</Label>
          <Input v-model="name" placeholder="Например, основной сервер" autocomplete="off" />
        </div>
        <Textarea
          v-model="content"
          rows="5"
          spellcheck="false"
          class="font-mono text-xs"
          placeholder="[Interface]&#10;PrivateKey = ...&#10;[Peer]&#10;PublicKey = ...&#10;Endpoint = host:51820"
        />
        <div class="flex flex-wrap items-center gap-2">
          <label
            class="inline-flex h-9 cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-3 text-sm hover:bg-accent"
          >
            <FileUp class="h-4 w-4" />
            Выбрать файл
            <input type="file" accept=".conf,.txt,text/plain" class="hidden" @change="pickFile" />
          </label>
          <Button size="sm" :disabled="busy || !name.trim() || !content.trim()" @click="addConfig">
            <Plus class="h-3.5 w-3.5" />
            Добавить конфиг
          </Button>
        </div>
      </div>
    </div>
  </Card>
</template>
