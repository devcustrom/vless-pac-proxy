<script setup>
import { computed, reactive, ref } from 'vue'
import { Field, Form, getInput, reset, setInput, useForm } from '@formisch/vue'
import { CheckCircle2, FileUp, FlaskConical, Plus, Shield, Trash2, Zap } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation, useWgConfigsQuery } from '@/lib/queries'
import { wgConfigSchema } from '@/lib/validators'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import Textarea from '@/components/ui/Textarea.vue'

const props = defineProps({ s: { type: Object, required: true } })

const query = useWgConfigsQuery()
const configs = computed(() => query.data.value?.configs || [])
const loading = computed(() => query.isLoading.value)
const loadError = computed(() => query.error.value?.message || '')
const tests = reactive({})
const active = computed(() => configs.value.find((item) => item.active))
const form = useForm({
  schema: wgConfigSchema,
  initialInput: { name: '', content: '' },
})

const keys = [['status'], ['wg-configs']]
const addAction = useActionMutation(
  (payload) => api('/api/wg/configs', { action: 'add', ...payload }),
  keys,
)
const useAction = useActionMutation(
  (slug) => api('/api/wg/configs', { action: 'use', slug }),
  keys,
)
const removeAction = useActionMutation(
  (slug) => api('/api/wg/configs', { action: 'remove', slug }),
  keys,
)
const addLoading = computed(() => addAction.isLoading.value)
const useLoading = computed(() => useAction.isLoading.value)
const removeLoading = computed(() => removeAction.isLoading.value)
const usingSlug = ref('')
const removingSlug = ref('')
const adding = computed(() => addAction.isLoading.value)

async function addConfig(output) {
  try {
    await addAction.mutateAsync(output)
    reset(form)
    notify('WireGuard-конфиг добавлен')
  } catch {
    /* тост показан */
  }
}

async function pickFile(event) {
  const file = event.target.files?.[0]
  if (!file) return
  const current = getInput(form)
  setInput(form, {
    input: {
      name: current.name.trim() || file.name.replace(/\.(conf|txt)$/i, ''),
      content: await file.text(),
    },
  })
  event.target.value = ''
  notify('Файл загружен — нажмите «Добавить»')
}

async function useConfig(item) {
  usingSlug.value = item.slug
  try {
    await useAction.mutateAsync(item.slug)
    notify('Активный WireGuard: ' + item.name)
  } catch {
    /* тост показан */
  } finally {
    usingSlug.value = ''
  }
}

async function removeConfig(item) {
  if (!confirm(`Удалить конфиг «${item.name}»?`)) return
  removingSlug.value = item.slug
  try {
    await removeAction.mutateAsync(item.slug)
    delete tests[item.slug]
    notify('Конфиг удалён')
  } catch {
    /* тост показан */
  } finally {
    removingSlug.value = ''
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

      <div v-if="loading" class="rounded-lg border px-3 py-4 text-sm text-muted-foreground">
        Загружаю конфиги…
      </div>
      <div v-else-if="loadError" class="space-y-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-3 text-sm text-destructive">
        <p>Не удалось загрузить конфиги: {{ loadError }}</p>
        <Button size="sm" variant="outline" @click="query.refetch()">Повторить</Button>
      </div>
      <ul v-else-if="configs.length" class="max-h-64 divide-y overflow-auto rounded-lg border scrollbar-thin">
        <li
          v-for="item in configs"
          :key="item.slug"
          class="px-3 py-2"
          :class="{ 'bg-success/5': item.active }"
        >
          <div class="flex flex-wrap items-center gap-2 text-sm">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium">{{ item.name }}</span>
                <Badge v-if="item.active" variant="success">активен</Badge>
                <Badge v-if="!item.valid" variant="warning">невалидный</Badge>
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
              :loading="usingSlug === item.slug"
              :disabled="useLoading || removeLoading || !item.valid"
              @click="useConfig(item)"
            >
              <Zap class="h-3.5 w-3.5" />
              Использовать
            </Button>
            <Button
              size="sm"
              variant="outline"
              :disabled="useLoading || removeLoading"
              :loading="tests[item.slug]?.busy"
              @click="testConfig(item)"
            >
              <FlaskConical class="h-3.5 w-3.5" />
              Тест
            </Button>
            <Button
              size="sm"
              variant="ghost"
              :disabled="useLoading || removeLoading"
              :loading="removingSlug === item.slug"
              @click="removeConfig(item)"
            >
              <Trash2 class="h-3.5 w-3.5" />
              Удалить
            </Button>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Конфигов нет. Добавьте первый — он станет активным.
      </p>

      <Form :of="form" class="space-y-2 rounded-lg border bg-muted/30 p-3" @submit="addConfig">
        <div class="flex items-center gap-2 text-sm font-medium">
          <Plus class="h-4 w-4" />
          Добавить конфиг
        </div>
        <Field v-slot="field" :of="form" :path="['name']">
          <div class="space-y-1">
            <Label>Имя</Label>
            <Input
              v-model="field.input"
              v-bind="field.props"
              placeholder="Например, основной сервер"
              autocomplete="off"
            />
            <p v-if="field.errors" class="text-xs text-destructive">{{ field.errors[0] }}</p>
          </div>
        </Field>
        <Field v-slot="field" :of="form" :path="['content']">
          <div class="space-y-1">
            <Label>Конфиг</Label>
            <Textarea
              v-model="field.input"
              v-bind="field.props"
              rows="5"
              spellcheck="false"
              class="font-mono text-xs"
              placeholder="[Interface]&#10;PrivateKey = ...&#10;[Peer]&#10;PublicKey = ...&#10;Endpoint = host:51820"
            />
            <p v-if="field.errors" class="text-xs text-destructive">{{ field.errors[0] }}</p>
          </div>
        </Field>
        <div class="flex flex-wrap items-center gap-2">
          <label
            class="inline-flex h-9 cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-3 text-sm hover:bg-accent"
          >
            <FileUp class="h-4 w-4" />
            Выбрать файл
            <input type="file" accept=".conf,.txt,text/plain" class="hidden" @change="pickFile" />
          </label>
          <Button type="submit" size="sm" :loading="adding">
            <Plus class="h-3.5 w-3.5" />
            Добавить конфиг
          </Button>
        </div>
      </Form>
    </div>
  </Card>
</template>
