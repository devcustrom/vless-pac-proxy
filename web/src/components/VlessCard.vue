<script setup>
import { computed, reactive, ref } from 'vue'
import { Field, Form, getInput, reset, setInput, useForm } from '@formisch/vue'
import {
  Activity,
  CheckCircle2,
  FileUp,
  FlaskConical,
  Plus,
  Trash2,
  Zap,
} from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { api, notify } from '@/lib/store'
import { useActionMutation, useXrayConfigsQuery } from '@/lib/queries'
import { xrayConfigSchema } from '@/lib/validators'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Dialog from '@/components/ui/Dialog.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import OverflowMenu from '@/components/ui/OverflowMenu.vue'
import Textarea from '@/components/ui/Textarea.vue'

defineProps({ s: { type: Object, required: true } })

const query = useXrayConfigsQuery()
const configs = computed(() => query.data.value?.configs || [])
const loading = computed(() => query.isLoading.value)
const loadError = computed(() => query.error.value?.message || '')
const active = computed(() => configs.value.find((item) => item.active))
const tests = reactive({})
const addOpen = ref(false)
const usingSlug = ref('')
const removingSlug = ref('')
const testingAll = ref(false)
const testedCount = computed(
  () => Object.values(tests).filter((item) => item && !item.busy).length,
)
const workingCount = computed(
  () => Object.values(tests).filter((item) => item?.ok).length,
)

const form = useForm({
  schema: xrayConfigSchema,
  initialInput: { name: '', link: '' },
})

const keys = [['status'], ['xray-configs']]
const addAction = useActionMutation(
  (payload) => api('/api/xray/configs', { action: 'add', ...payload }),
  keys,
)
const useAction = useActionMutation(
  (slug) => api('/api/xray/configs', { action: 'use', slug }),
  keys,
)
const removeAction = useActionMutation(
  (slug) => api('/api/xray/configs', { action: 'remove', slug }),
  keys,
)
const adding = computed(() => addAction.isLoading.value)
const useLoading = computed(() => useAction.isLoading.value)
const removeLoading = computed(() => removeAction.isLoading.value)

function protocolLabel(protocol) {
  if (protocol === 'shadowsocks') return 'Outline / SS'
  return String(protocol || '').toUpperCase()
}

async function addConfig(output) {
  try {
    const result = await addAction.mutateAsync(output)
    reset(form)
    addOpen.value = false
    notify(
      result.active
        ? 'Xray-конфиг добавлен и активен'
        : 'Xray-конфиг добавлен. Нажмите «Использовать», чтобы переключиться',
    )
  } catch {
    /* тост уже показан */
  }
}

async function pickFile(event) {
  const file = event.target.files?.[0]
  if (!file) return
  const current = getInput(form)
  setInput(form, {
    input: {
      name: current.name.trim() || file.name.replace(/\.(txt|json|conf)$/i, ''),
      link: await file.text(),
    },
  })
  event.target.value = ''
  notify('Файл загружен — нажмите «Добавить»')
}

async function useConfig(item) {
  usingSlug.value = item.slug
  try {
    await useAction.mutateAsync(item.slug)
    notify('Активный Xray: ' + item.name)
  } catch {
    /* тост уже показан */
  } finally {
    usingSlug.value = ''
  }
}

async function removeConfig(item) {
  if (!confirm(`Удалить конфиг «${item.name}»?`)) return
  removingSlug.value = item.slug
  try {
    await removeAction.mutateAsync(item.slug)
    notify('Xray-конфиг удалён')
  } catch {
    /* тост уже показан */
  } finally {
    removingSlug.value = ''
  }
}

async function testConfig(item) {
  tests[item.slug] = { busy: true }
  // Автовыбор тоже запускает тестовый Xray. Если слот занят — ждём и
  // повторяем, чтобы «Проверить все» не сдавался на первой же гонке.
  for (let attempt = 0; attempt < 40; attempt += 1) {
    try {
      const result = await api('/api/xray/configs', { action: 'test', slug: item.slug })
      tests[item.slug] = { ok: true, ip: result.ip, ms: result.ms }
      return
    } catch (error) {
      const busy = String(error.message || '').includes('уже идёт')
      if (!busy || attempt === 39) {
        tests[item.slug] = { ok: false, error: error.message }
        return
      }
      await new Promise((resolve) => setTimeout(resolve, 1500))
    }
  }
}

async function testAll() {
  if (testingAll.value) return
  const targets = configs.value.filter((item) => item.valid)
  if (!targets.length) {
    notify('Нет конфигов для проверки', 'error')
    return
  }
  testingAll.value = true
  try {
    for (const item of targets) {
      await testConfig(item)
    }
    notify(`Проверка завершена: рабочих ${workingCount.value} из ${targets.length}`)
  } finally {
    testingAll.value = false
  }
}
</script>

<template>
  <Card
    data-testid="xray-configs"
    title="Xray: VLESS / Outline"
    description="Сохранённые VLESS, VMess, Trojan и Shadowsocks/Outline. Активный конфиг использует канал Xray."
  >
    <template #icon>
      <Activity class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="s.vlessConfigured ? 'success' : 'secondary'">
          {{ s.vlessConfigured ? 'настроен' : 'не настроен' }}
        </Badge>
        <span class="text-muted-foreground">
          активный: {{ active?.name || '—' }}
          <template v-if="active">
            · {{ protocolLabel(active.protocol) }} {{ active.address }}:{{ active.port }}
          </template>
        </span>
        <Button
          size="sm"
          variant="outline"
          class="ml-auto"
          :loading="testingAll"
          :disabled="testingAll || !configs.length"
          @click="testAll"
        >
          <FlaskConical class="h-3.5 w-3.5" />
          Проверить все
        </Button>
        <Button size="sm" variant="outline" @click="addOpen = true">
          <Plus class="h-3.5 w-3.5" />
          Добавить конфиг
        </Button>
      </div>

      <p v-if="testedCount" class="text-xs text-muted-foreground">
        Проверено {{ testedCount }} · рабочих {{ workingCount }}
      </p>

      <div v-if="loading" class="rounded-lg border px-3 py-4 text-sm text-muted-foreground">
        Загружаю конфиги…
      </div>
      <div
        v-else-if="loadError"
        class="space-y-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-3 text-sm text-destructive"
      >
        <p>Не удалось загрузить конфиги: {{ loadError }}</p>
        <Button size="sm" variant="outline" @click="query.refetch()">Повторить</Button>
      </div>
      <ul
        v-else-if="configs.length"
        class="max-h-72 divide-y overflow-auto rounded-lg border scrollbar-thin"
      >
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
                <Badge variant="outline">{{ protocolLabel(item.protocol) }}</Badge>
                <Badge v-if="item.dynamic" variant="secondary">динамический</Badge>
                <Badge v-if="!item.valid" variant="warning">невалидный</Badge>
              </div>
              <div class="mt-0.5 break-all text-xs text-muted-foreground">
                {{ item.address || '—' }}:{{ item.port || '—' }}
              </div>
              <div v-if="tests[item.slug]" class="mt-1 text-xs">
                <span v-if="tests[item.slug].busy" class="text-muted-foreground">
                  проверяю трафик…
                </span>
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
            <OverflowMenu label="Действия с конфигом">
              <button
                type="button"
                class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
                :disabled="useLoading || removeLoading || !item.valid"
                @click="testConfig(item)"
              >
                <FlaskConical class="h-3.5 w-3.5" />
                {{ tests[item.slug]?.busy ? 'Проверяю…' : 'Проверить трафик' }}
              </button>
              <button
                type="button"
                class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs text-destructive hover:bg-destructive/10 disabled:opacity-50"
                :disabled="useLoading || removeLoading"
                @click="removeConfig(item)"
              >
                <Trash2 class="h-3.5 w-3.5" />
                Удалить
              </button>
            </OverflowMenu>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">
        Конфигов нет. Добавьте первый — он станет активным.
      </p>

      <p class="text-xs text-muted-foreground">
        Подписки и пул конфигов — в карточке пула на этой вкладке. Поддерживаются
        статические ссылки, <code class="rounded bg-muted px-1 py-0.5">ssconf://…#name</code>
        и <code class="rounded bg-muted px-1 py-0.5">https://…json</code> Outline.
      </p>
      <RouterLink to="/routing" class="inline-flex text-xs text-primary hover:underline">
        Настроить маршрутизацию доменов →
      </RouterLink>
    </div>

    <Dialog
      :open="addOpen"
      title="Добавить Xray-конфиг"
      description="Вставьте ссылку, JSON Outline или выберите файл. Новый конфиг не меняет активный, пока вы не нажмёте «Использовать»."
      @close="addOpen = false"
    >
      <Form :of="form" class="space-y-2" @submit="addConfig">
        <Field v-slot="field" :of="form" :path="['name']">
          <div class="space-y-1">
            <Label>Имя</Label>
            <Input
              v-model="field.input"
              v-bind="field.props"
              placeholder="Например, основной VLESS"
              autocomplete="off"
            />
            <p v-if="field.errors" class="text-xs text-destructive">{{ field.errors[0] }}</p>
          </div>
        </Field>
        <Field v-slot="field" :of="form" :path="['link']">
          <div class="space-y-1">
            <Label>Ссылка или JSON</Label>
            <Textarea
              v-model="field.input"
              v-bind="field.props"
              rows="4"
              spellcheck="false"
              class="font-mono text-xs"
              placeholder="vless://… / ss://…@host:port/?outline=1#name / ssconf://…#name / https://…json"
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
            <input
              type="file"
              accept=".txt,.json,.conf,text/plain,application/json"
              class="hidden"
              @change="pickFile"
            />
          </label>
          <Button type="submit" size="sm" :loading="adding">
            <Plus class="h-3.5 w-3.5" />
            Добавить конфиг
          </Button>
        </div>
      </Form>
    </Dialog>
  </Card>
</template>
