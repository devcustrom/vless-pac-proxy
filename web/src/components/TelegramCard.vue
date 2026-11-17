<script setup>
import { computed, reactive, ref, watch } from 'vue'
import { Copy, ExternalLink, MessageCircle, RefreshCw, Send, ShieldAlert } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import Select from '@/components/ui/Select.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const t = computed(() => props.s.telegram || {})
const enabled = computed(() => !!props.s.plugins?.telegram)
const running = computed(() => !!t.value.running)
const installed = computed(() => !!t.value.installed)
const isWindows = computed(() => t.value.platform === 'windows')
const sidecarName = computed(() => (isWindows.value ? 'tgws-proxy.exe' : 'tgws-proxy'))
const buildScript = computed(() => (isWindows.value ? 'sidecar/tgws/build.ps1' : 'sidecar/tgws/build.sh'))
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

const orderLabels = {
  direct: 'напрямую',
  wg: 'WireGuard',
  vless: 'VLESS',
}
const activeOrder = computed(() => t.value.order || [])
const directResult = computed(() => t.value.probe?.direct || {})
const stats = computed(() => t.value.stats || {})
const upstream = computed(() => t.value.upstream || {})

function fmtBytes(value) {
  const n = Number(value || 0)
  if (n < 1024) return `${n} Б`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} КБ`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} МБ`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} ГБ`
}

async function enable() {
  try {
    await enableAction.mutateAsync()
    notify('Telegram-прокси включён')
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
    await restartAction.mutateAsync()
    notify('Telegram-прокси перезапускается')
  } catch {
    /* тост показан */
  }
}

async function save() {
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
          Соберите <code>{{ buildScript }}</code> и положите
          <code>{{ sidecarName }}</code> в каталог данных приложения. После этого
          включите плагин. Если sidecar опубликован в релизе, его можно
          установить кнопкой ниже.
        </p>
        <Button size="sm" variant="outline" :loading="updating" @click="checkUpdate">
          Скачать / обновить sidecar
        </Button>
      </div>

      <div class="flex flex-wrap gap-2">
        <Button v-if="!enabled" size="sm" :loading="enabling" @click="enable">
          Включить Telegram-прокси
        </Button>
        <Button v-else size="sm" variant="outline" :loading="disabling" @click="disable">
          Выключить
        </Button>
        <Button
          v-if="enabled"
          size="sm"
          variant="outline"
          :loading="restarting"
          @click="restart"
        >
          <RefreshCw class="h-3.5 w-3.5" />
          Перезапустить
        </Button>
        <Button
          v-if="running"
          size="sm"
          variant="outline"
          :loading="busyLink"
          @click="copyLink"
        >
          <Copy class="h-3.5 w-3.5" />
          Копировать ссылку
        </Button>
        <Button
          v-if="running"
          size="sm"
          variant="outline"
          :loading="busyLink"
          @click="openLink"
        >
          <ExternalLink class="h-3.5 w-3.5" />
          Открыть в Telegram
        </Button>
        <Button size="sm" variant="ghost" :loading="probing" @click="probe">
          <MessageCircle class="h-3.5 w-3.5" />
          Проверить прокси
        </Button>
      </div>

      <div v-if="t.secretMasked" class="flex flex-wrap items-center gap-2 text-xs">
        <span class="text-muted-foreground">Secret:</span>
        <code class="rounded bg-muted px-1.5 py-0.5">{{ t.secretMasked }}</code>
        <Button size="sm" variant="ghost" :loading="rotating" @click="rotateSecret">
          Сгенерировать новый
        </Button>
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
            @update:model-value="(v) => { form.tryDirect = v; markDirty() }"
          />
        </div>

        <div class="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-end">
          <div class="space-y-1">
            <Label>Порядок транспортов</Label>
            <Select
              v-model="form.order"
              :options="orderOptions"
              @update:model-value="markDirty"
            />
          </div>
          <Button size="sm" variant="outline" :loading="saving" :disabled="!dirty" @click="save">
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

        <div class="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="ghost" :loading="probingDirect" @click="probeDirect">
            Проверить напрямую
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

      <div class="grid gap-3 sm:grid-cols-2">
        <div class="space-y-1">
          <Label>Порт</Label>
          <Input
            v-model="form.port"
            inputmode="numeric"
            maxlength="5"
            placeholder="1443"
            @input="markDirty"
          />
        </div>
        <div class="space-y-1">
          <Label>Fake TLS domain (необязательно)</Label>
          <Input
            v-model="form.fakeTls"
            placeholder="example.com"
            autocomplete="off"
            @input="markDirty"
          />
        </div>
        <div class="space-y-1">
          <Label>CF Worker domain</Label>
          <Input
            v-model="form.cfWorker"
            placeholder="worker.example.workers.dev"
            autocomplete="off"
            @input="markDirty"
          />
        </div>
        <div class="space-y-1">
          <Label>CF Proxy domain</Label>
          <Input
            v-model="form.cfProxy"
            placeholder="example.com"
            autocomplete="off"
            @input="markDirty"
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
          :disabled="form.order !== 'direct-only'"
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
          <Button size="sm" variant="outline" :loading="updating" @click="checkUpdate">
            Проверить обновление
          </Button>
          <Switch
            :model-value="form.autoUpdate"
            @update:model-value="(v) => { form.autoUpdate = v; markDirty() }"
          />
        </div>
      </div>

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
  </Card>
</template>
