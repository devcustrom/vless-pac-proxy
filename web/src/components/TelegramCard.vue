<script setup>
import { computed, reactive, ref, watch } from 'vue'
import { Copy, ExternalLink, MessageCircle, RefreshCw, Send, ShieldAlert } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import { digitsOnly, normalizeDomainInput } from '@/lib/validators'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import OverflowMenu from '@/components/ui/OverflowMenu.vue'
import Select from '@/components/ui/Select.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const t = computed(() => props.s.telegram || {})
const enabled = computed(() => !!props.s.plugins?.telegram)
const running = computed(() => !!t.value.running)
const installed = computed(() => !!t.value.installed)
const busyLink = ref(false)
const dirty = ref(false)

const orderOptions = [
  { value: 'auto', label: 'WS → VPN → напрямую' },
  { value: 'vpn-first', label: 'VPN → WS → напрямую' },
  { value: 'direct-only', label: 'Только напрямую' },
  { value: 'vpn-only', label: 'Только VPN' },
]

const form = reactive({
  port: '1443',
  tryDirect: true,
  order: 'auto',
  cfWorker: '',
  cfProxy: '',
  fakeTls: '',
  h2: false,
  autoUpdate: true,
})

function syncForm() {
  if (dirty.value) return
  form.port = String(t.value.port || 1443)
  form.tryDirect = t.value.tryDirect !== false
  form.order = t.value.orderPreset || 'auto'
  form.cfWorker = t.value.cfWorker || ''
  form.cfProxy = t.value.cfProxy || ''
  form.fakeTls = t.value.fakeTls || ''
  form.h2 = !!t.value.h2
  form.autoUpdate = t.value.autoUpdate !== false
}

watch(t, syncForm, { immediate: true, deep: true })

function markDirty() {
  dirty.value = true
}

watch(
  () => form.order,
  (value) => {
    if (value === 'direct-only' && !form.tryDirect) {
      form.tryDirect = true
      markDirty()
    }
  },
)

const setAction = useActionMutation(
  (payload) => api('/api/telegram', { action: 'set', ...payload }),
  [['status']],
)
const enableAction = useActionMutation(
  () => api('/api/telegram', { action: 'enable' }),
  [['status']],
)
const disableAction = useActionMutation(
  () => api('/api/telegram', { action: 'disable' }),
  [['status']],
)
const restartAction = useActionMutation(
  () => api('/api/telegram', { action: 'restart' }),
  [['status']],
)
const probeAction = useActionMutation(
  () => api('/api/telegram', { action: 'probe' }),
  [['status']],
)
const probeDirectAction = useActionMutation(
  () => api('/api/telegram', { action: 'probe-direct' }),
  [['status']],
)
const rotateAction = useActionMutation(
  () => api('/api/telegram', { action: 'rotate-secret' }),
  [['status']],
)
const updateAction = useActionMutation(
  () => api('/api/telegram', { action: 'check-update' }),
  [['status']],
)

const saving = computed(() => setAction.isLoading.value)
const enabling = computed(() => enableAction.isLoading.value)
const disabling = computed(() => disableAction.isLoading.value)
const restarting = computed(() => restartAction.isLoading.value)
const probing = computed(() => probeAction.isLoading.value)
const probingDirect = computed(() => probeDirectAction.isLoading.value)
const rotating = computed(() => rotateAction.isLoading.value)
const updating = computed(() => updateAction.isLoading.value || !!t.value.update?.checking)
const updateState = computed(() => t.value.update || {})
const anyBusy = computed(
  () =>
    saving.value ||
    enabling.value ||
    disabling.value ||
    restarting.value ||
    probing.value ||
    probingDirect.value ||
    rotating.value ||
    updating.value,
)
const portValid = computed(() => {
  const port = Number(form.port)
  return Number.isInteger(port) && port >= 1 && port <= 65535
})

const orderLabels = {
  direct: 'напрямую',
  wg: 'WireGuard',
  vless: 'VLESS',
}
const activeOrder = computed(() => t.value.order || [])
const directResult = computed(() => t.value.probe?.direct || {})
const stats = computed(() => t.value.stats || {})
const upstream = computed(() => t.value.upstream || {})
const health = computed(() => t.value.health || {})
const healthDcs = computed(() => (Array.isArray(health.value.dcs) ? health.value.dcs : []))
const healthTcp = computed(() => (Array.isArray(health.value.tcp) ? health.value.tcp : []))
const healthTcpOk = computed(() => healthTcp.value.filter((item) => item.ok).length)
const healthTcpFailed = computed(() =>
  healthTcp.value
    .filter((item) => !item.ok)
    .map((item) => `DC${item.dc}`)
    .join(', '),
)
const healthAt = computed(() => {
  const at = Number(health.value.at || 0)
  return at > 0 ? new Date(at * 1000).toLocaleTimeString('ru-RU') : ''
})

function fmtBytes(value) {
  const n = Number(value || 0)
  if (n < 1024) return `${n} Б`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} КБ`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} МБ`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} ГБ`
}

async function enable() {
  try {
    const result = await enableAction.mutateAsync()
    if (result?.error) {
      notify(`Telegram-прокси включён, но не запустился: ${result.error}`, 'error')
    } else if (result?.running === false) {
      notify('Telegram-прокси включён, но пока не отвечает', 'error')
    } else {
      notify('Telegram-прокси включён')
    }
  } catch {
    /* тост показан */
  }
}

async function disable() {
  try {
    await disableAction.mutateAsync()
    notify('Telegram-прокси выключен')
  } catch {
    /* тост показан */
  }
}

async function restart() {
  try {
    const result = await restartAction.mutateAsync()
    if (result?.error) {
      notify(`Перезапуск не удался: ${result.error}`, 'error')
    } else {
      notify('Telegram-прокси перезапускается')
    }
  } catch {
    /* тост показан */
  }
}

function onHostPaste(event, key) {
  const text = event.clipboardData?.getData('text') || ''
  const cleaned = normalizeDomainInput(text)
  if (!cleaned) return
  event.preventDefault()
  form[key] = cleaned
  markDirty()
}

async function save() {
  if (!portValid.value) {
    notify('Порт должен быть числом 1..65535', 'error')
    return
  }
  form.fakeTls = normalizeDomainInput(form.fakeTls)
  form.cfWorker = normalizeDomainInput(form.cfWorker)
  form.cfProxy = normalizeDomainInput(form.cfProxy)
  try {
    await setAction.mutateAsync({
      port: Number(form.port),
      tryDirect: form.tryDirect,
      order: form.order,
      cfWorker: form.cfWorker,
      cfProxy: form.cfProxy,
      fakeTls: form.fakeTls,
      h2: form.h2,
      autoUpdate: form.autoUpdate,
    })
    dirty.value = false
    notify('Настройки Telegram-прокси сохранены')
  } catch {
    /* тост показан */
  }
}

async function probe() {
  try {
    const result = await probeAction.mutateAsync()
    notify(result.probe?.running ? 'Прокси отвечает' : 'Прокси не отвечает')
  } catch {
    /* тост показан */
  }
}

async function probeDirect() {
  try {
    const result = await probeDirectAction.mutateAsync()
    const ok = Object.values(result.probe?.direct || {}).filter(Boolean).length
    notify(`Напрямую доступно DC: ${ok}/6`)
  } catch {
    /* тост показан */
  }
}

async function rotateSecret() {
  if (!confirm('Сгенерировать новый секрет? Старая tg://-ссылка перестанет работать.')) return
  try {
    await rotateAction.mutateAsync()
    notify('Секрет обновлён — скопируйте новую ссылку')
  } catch {
    /* тост показан */
  }
}

async function checkUpdate() {
  try {
    const result = await updateAction.mutateAsync()
    if (result.published === false) {
      notify(result.message || 'Манифест обновлений ещё не опубликован')
    } else {
      notify(`Sidecar обновлён/актуален: ${result.version || 'ok'}`)
    }
  } catch {
    /* тост показан */
  }
}

async function getLink() {
  const result = await api('/api/telegram/link', {})
  return result.link
}

async function copyLink() {
  busyLink.value = true
  try {
    const link = await getLink()
    await navigator.clipboard.writeText(link)
    notify('Ссылка скопирована')
  } catch (error) {
    notify(error.message || 'Не удалось получить ссылку', 'error')
  } finally {
    busyLink.value = false
  }
}

async function openLink() {
  busyLink.value = true
  try {
    const link = await getLink()
    window.location.href = link
  } catch (error) {
    notify(error.message || 'Не удалось открыть Telegram', 'error')
  } finally {
    busyLink.value = false
  }
}
</script>

<template>
  <Card
    title="Telegram-прокси"
    description="Локальный MTProto-прокси: WS-транспорт tg-ws-proxy, затем WireGuard/VLESS, затем напрямую."
  >
    <template #icon>
      <Send class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-5">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="enabled ? 'success' : 'secondary'">
          {{ enabled ? 'плагин включён' : 'плагин выключен' }}
        </Badge>
        <Badge :variant="running ? 'success' : 'destructive'">
          {{ running ? 'работает' : 'остановлен' }}
        </Badge>
        <Badge :variant="installed ? 'outline' : 'warning'">
          {{ installed ? 'sidecar установлен' : 'sidecar не найден' }}
        </Badge>
        <span v-if="t.version" class="text-muted-foreground">v{{ t.version }}</span>
        <span v-if="t.installedVersion" class="text-muted-foreground">
          сборка {{ t.installedVersion }}
        </span>
        <span class="text-muted-foreground">{{ t.host }}:{{ t.port }}</span>
      </div>

      <div v-if="t.error" class="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
        {{ t.error }}
      </div>

      <div
        v-if="!installed"
        class="space-y-1 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400"
      >
        <div class="flex items-center gap-2 font-medium">
          <ShieldAlert class="h-4 w-4" />
          Нужен headless-компонент
        </div>
        <p>
          Нажмите кнопку — приложение скачает готовый sidecar из релиза,
          проверит sha256 и положит его в каталог данных. После этого включите
          плагин.
        </p>
        <Button size="sm" variant="outline" :loading="updating" :disabled="anyBusy" @click="checkUpdate">
          Скачать / обновить sidecar
        </Button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <Button
          v-if="running"
          size="sm"
          :loading="busyLink"
          :disabled="anyBusy"
          @click="openLink"
        >
          <ExternalLink class="h-3.5 w-3.5" />
          Открыть в Telegram
        </Button>
        <Button
          v-else-if="enabled"
          size="sm"
          :loading="restarting"
          :disabled="anyBusy"
          @click="restart"
        >
          Запустить прокси
        </Button>
        <OverflowMenu label="Действия Telegram">
          <button
            v-if="running"
            type="button"
            class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
            :disabled="anyBusy"
            @click="copyLink"
          >
            <Copy class="h-3.5 w-3.5" />
            Копировать ссылку
          </button>
          <button
            v-if="enabled"
            type="button"
            class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
            :disabled="anyBusy"
            @click="restart"
          >
            <RefreshCw class="h-3.5 w-3.5" />
            Перезапустить
          </button>
          <button
            type="button"
            class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent disabled:opacity-50"
            :disabled="anyBusy"
            @click="probe"
          >
            <MessageCircle class="h-3.5 w-3.5" />
            Проверить прокси
          </button>
          <button
            v-if="t.secretMasked"
            type="button"
            class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs text-destructive hover:bg-destructive/10 disabled:opacity-50"
            :disabled="anyBusy"
            @click="rotateSecret"
          >
            <RefreshCw class="h-3.5 w-3.5" />
            Сменить секрет
          </button>
          <button
            v-if="enabled"
            type="button"
            class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs text-destructive hover:bg-destructive/10 disabled:opacity-50"
            :disabled="anyBusy"
            @click="disable"
          >
            Выключить плагин
          </button>
        </OverflowMenu>
      </div>

      <div v-if="t.secretMasked" class="flex flex-wrap items-center gap-2 text-xs">
        <span class="text-muted-foreground">Secret:</span>
        <code class="rounded bg-muted px-1.5 py-0.5">{{ t.secretMasked }}</code>
      </div>

      <div class="space-y-3 rounded-lg border bg-muted/30 p-3">
        <div class="flex items-center justify-between gap-3">
          <div>
            <div class="text-sm font-medium">Пробовать напрямую</div>
            <p class="text-xs text-muted-foreground">
              Разрешает direct-пункты в порядке: WS напрямую и TCP напрямую.
              Если выключено, используются только WireGuard/VLESS.
            </p>
          </div>
          <Switch
            :model-value="form.tryDirect"
            :disabled="saving || form.order === 'direct-only'"
            @update:model-value="(v) => { form.tryDirect = v; markDirty() }"
          />
        </div>
        <p v-if="form.order === 'direct-only'" class="text-xs text-muted-foreground">
          Для режима «только напрямую» direct включён принудительно.
        </p>

        <div class="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-end">
          <div class="space-y-1">
            <Label>Порядок транспортов</Label>
            <Select
              v-model="form.order"
              :options="orderOptions"
              :disabled="saving"
              @update:model-value="markDirty"
            />
          </div>
          <Button size="sm" variant="outline" :loading="saving" :disabled="!dirty || !portValid || anyBusy" @click="save">
            Применить
          </Button>
        </div>

        <div class="flex flex-wrap items-center gap-1.5 text-xs">
          <span class="text-muted-foreground">Порядок:</span>
          <Badge
            v-for="item in activeOrder"
            :key="item"
            :variant="item === 'direct' ? 'outline' : 'secondary'"
          >
            {{ orderLabels[item] || item }}
          </Badge>
          <span v-if="!activeOrder.length" class="text-destructive">не настроен</span>
        </div>

        <div class="rounded-lg border bg-background/40 p-2.5 text-xs">
          <div class="flex items-center justify-between gap-2">
            <span class="font-medium">Фоновая проверка DC</span>
            <span v-if="healthAt" class="text-muted-foreground">{{ healthAt }}</span>
          </div>
          <div v-if="healthDcs.length" class="mt-1.5 flex flex-wrap gap-1.5">
            <Badge
              v-for="item in healthDcs"
              :key="`${item.dc}-${item.media ? 'media' : 'normal'}`"
              :variant="item.ok ? 'success' : 'destructive'"
              :title="item.error || `${item.domain} → ${item.target}`"
            >
              DC{{ item.dc }}{{ item.media ? ' media' : '' }}:
              {{ item.ok ? `${item.latencyMs} мс` : 'нет' }}
            </Badge>
          </div>
          <p v-else class="mt-1 text-muted-foreground">
            Данных пока нет — проверка запускается вместе с прокси.
          </p>
          <p v-if="healthTcp.length" class="mt-1 text-muted-foreground">
            TCP напрямую: {{ healthTcpOk }}/{{ healthTcp.length }}
            <span v-if="healthTcpFailed">· недоступны {{ healthTcpFailed }}</span>
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="ghost" :loading="probingDirect" :disabled="anyBusy" @click="probeDirect">
            Проверить TCP сейчас
          </Button>
          <span v-if="t.probe?.at" class="text-xs text-muted-foreground">
            {{ new Date(t.probe.at).toLocaleTimeString('ru-RU') }}
          </span>
          <Badge
            v-for="(ok, dc) in directResult"
            :key="dc"
            :variant="ok ? 'success' : 'destructive'"
          >
            DC{{ dc }}: {{ ok ? 'доступен' : 'нет' }}
          </Badge>
        </div>
      </div>

      <details class="rounded-lg border bg-muted/30 px-3 py-2">
        <summary class="cursor-pointer text-sm font-medium">Дополнительно: сеть и sidecar</summary>
        <div class="mt-3 space-y-3">
      <div class="grid gap-3 sm:grid-cols-2">
        <div class="space-y-1">
          <Label>Порт</Label>
          <Input
            v-model="form.port"
            data-testid="telegram-port"
            inputmode="numeric"
            maxlength="5"
            placeholder="1443"
            :disabled="saving"
            @input="form.port = digitsOnly(form.port); markDirty()"
          />
        </div>
        <div class="space-y-1">
          <Label>Fake TLS domain (необязательно)</Label>
          <Input
            v-model="form.fakeTls"
            placeholder="example.com или ссылка из браузера"
            autocomplete="off"
            :disabled="saving"
            @input="markDirty"
            @paste="onHostPaste($event, 'fakeTls')"
            @blur="form.fakeTls = normalizeDomainInput(form.fakeTls); markDirty()"
          />
        </div>
        <div class="space-y-1">
          <Label>CF Worker domain</Label>
          <Input
            v-model="form.cfWorker"
            placeholder="worker.example.workers.dev или ссылка"
            autocomplete="off"
            :disabled="saving"
            @input="markDirty"
            @paste="onHostPaste($event, 'cfWorker')"
            @blur="form.cfWorker = normalizeDomainInput(form.cfWorker); markDirty()"
          />
        </div>
        <div class="space-y-1">
          <Label>CF Proxy domain</Label>
          <Input
            v-model="form.cfProxy"
            placeholder="example.com или ссылка из браузера"
            autocomplete="off"
            :disabled="saving"
            @input="markDirty"
            @paste="onHostPaste($event, 'cfProxy')"
            @blur="form.cfProxy = normalizeDomainInput(form.cfProxy); markDirty()"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border px-3 py-2">
        <div>
          <div class="text-sm font-medium">HTTP/2 media</div>
          <p class="text-xs text-muted-foreground">
            Работает только в режиме «только напрямую»; для VPN-путей отключается.
          </p>
        </div>
        <Switch
          :model-value="form.h2"
          :disabled="saving || form.order !== 'direct-only'"
          @update:model-value="(v) => { form.h2 = v; markDirty() }"
        />
      </div>

      <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border px-3 py-2">
        <div class="min-w-0">
          <div class="text-sm font-medium">Автообновление sidecar</div>
          <p class="text-xs text-muted-foreground">
            Проверка манифеста раз в 6 часов. Новая версия скачивается с
            проверкой sha256, sidecar перезапускается автоматически.
          </p>
          <p v-if="updateState.lastError" class="mt-1 text-xs text-destructive">
            {{ updateState.lastError }}
          </p>
          <p v-else-if="updateState.lastCheck" class="mt-1 text-xs text-muted-foreground">
            Последняя проверка: {{ new Date(updateState.lastCheck).toLocaleString('ru-RU') }}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Button size="sm" variant="outline" :loading="updating" :disabled="anyBusy" @click="checkUpdate">
            Проверить обновление
          </Button>
          <Switch
            :model-value="form.autoUpdate"
            :disabled="saving"
            @update:model-value="(v) => { form.autoUpdate = v; markDirty() }"
          />
        </div>
      </div>
        </div>
      </details>

      <details class="rounded-lg border bg-muted/30 px-3 py-2">
        <summary class="cursor-pointer text-sm font-medium">Диагностика</summary>
        <div class="mt-3 space-y-3">
      <div class="grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
        <div class="rounded-lg border px-2 py-2">
          <div class="text-muted-foreground">Активных</div>
          <div class="font-medium">{{ stats.connectionsActive ?? 0 }}</div>
        </div>
        <div class="rounded-lg border px-2 py-2">
          <div class="text-muted-foreground">Всего</div>
          <div class="font-medium">{{ stats.connectionsTotal ?? 0 }}</div>
        </div>
        <div class="rounded-lg border px-2 py-2">
          <div class="text-muted-foreground">Отправлено</div>
          <div class="font-medium">{{ fmtBytes(stats.bytesUp) }}</div>
        </div>
        <div class="rounded-lg border px-2 py-2">
          <div class="text-muted-foreground">Получено</div>
          <div class="font-medium">{{ fmtBytes(stats.bytesDown) }}</div>
        </div>
      </div>

      <div v-if="upstream.last_path || upstream.last_errors" class="space-y-1 text-xs text-muted-foreground">
        <div v-if="upstream.last_path">Последний путь: <code>{{ upstream.last_path }}</code></div>
        <div v-for="(err, path) in (upstream.last_errors || {})" :key="path">
          {{ path }}: {{ err }}
        </div>
      </div>

      <p class="text-xs text-muted-foreground">
        В Telegram Desktop: Настройки → Продвинутые → Тип соединения → Прокси →
        MTProto, либо откройте ссылку кнопкой выше. Системный прокси Windows для
        Telegram не используется.
      </p>
        </div>
      </details>
    </div>
  </Card>
</template>
