<script setup>
import { computed, ref, watch } from 'vue'
import {
  Download,
  ExternalLink,
  FlaskConical,
  FolderOpen,
  RotateCw,
  ShieldAlert,
  ShieldCheck,
  Square,
  Trash2,
} from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Select from '@/components/ui/Select.vue'

const props = defineProps({ s: { type: Object, required: true } })

const z = computed(() => props.s.zapret || {})
const supported = computed(() => z.value.supported !== false)
const bundle = computed(() => !!z.value.bundleInstalled)
const service = computed(() => z.value.service || {})
const installed = computed(() => !!service.value.installed)
const running = computed(() => !!service.value.running)
const auto = computed(() => z.value.autoTune || {})
const domains = computed(() => z.value.domains || [])
const probeResult = ref(null)

const presetOptions = computed(() =>
  (z.value.presets || []).map((item) => ({ value: item.name, label: item.label })),
)
const manualPreset = ref('')
watch(
  () => z.value.preset,
  (value) => {
    if (!manualPreset.value) {
      manualPreset.value = value || presetOptions.value[0]?.value || ''
    }
  },
  { immediate: true },
)

const activeCatalog = computed(
  () => (z.value.catalog?.strategies || []).find((item) => item.active) || null,
)
const strategyLabel = computed(() => {
  if (activeCatalog.value) return activeCatalog.value.name || activeCatalog.value.id
  if (z.value.preset) return z.value.preset.replace(/\.(cmd|bat)$/i, '')
  return 'автоматическая'
})

const downloadAction = useActionMutation(
  (variant) => api('/api/zapret', { action: 'download', variant }),
  [['status']],
)
const installAction = useActionMutation(
  (presets) => api('/api/zapret', { action: 'install', presets }),
  [['status']],
)
const autoAction = useActionMutation(
  () => api('/api/zapret', { action: 'auto-tune' }),
  [['status']],
)
const startAction = useActionMutation(
  () => api('/api/zapret', { action: 'start' }),
  [['status']],
)
const stopAction = useActionMutation(
  () => api('/api/zapret', { action: 'stop' }),
  [['status']],
)
const probeAction = useActionMutation(
  () => api('/api/zapret', { action: 'probe' }),
  [['status']],
)
const openAction = useActionMutation(
  () => api('/api/zapret', { action: 'open-folder' }),
  [['status']],
)
const removeAction = useActionMutation(
  () => api('/api/zapret', { action: 'remove' }),
  [['status']],
)

const autoRunning = computed(() => !!auto.value.running || autoAction.isLoading.value)
const busy = computed(
  () =>
    downloadAction.isLoading.value ||
    installAction.isLoading.value ||
    autoAction.isLoading.value ||
    startAction.isLoading.value ||
    stopAction.isLoading.value ||
    probeAction.isLoading.value ||
    openAction.isLoading.value ||
    removeAction.isLoading.value,
)

const phaseLabel = computed(() => {
  if (auto.value.phase === 'installing') return 'Устанавливаю стратегию…'
  if (auto.value.phase === 'starting') return 'Запускаю обход…'
  return 'Ищу рабочую стратегию…'
})

const statusTitle = computed(() => {
  if (autoRunning.value) return 'Подбираю стратегию'
  if (running.value) return 'Обход DPI включён'
  if (installed.value) return 'Обход DPI выключен'
  if (!bundle.value) return 'Zapret не установлен'
  return 'Обход DPI не настроен'
})

const statusText = computed(() => {
  if (autoRunning.value) {
    return `${phaseLabel.value} Проверяю ${domains.value.length || 2} домена. Подтвердите запрос UAC — это может занять пару минут.`
  }
  if (running.value) {
    return `Стратегия: ${strategyLabel.value}. YouTube, Discord и указанные ниже домены должны открываться напрямую.`
  }
  if (installed.value) {
    return 'Служба уже установлена. Нажмите «Включить», чтобы запустить обход.'
  }
  if (!bundle.value) {
    return 'Скачаем Zapret, подберём рабочую стратегию и включим обход. Один раз нужно подтвердить UAC.'
  }
  return 'Нажмите «Включить обход DPI»: приложение подберёт стратегию, установит службу и запустит её.'
})

const primary = computed(() => {
  if (autoRunning.value) {
    return { label: phaseLabel.value, loading: true, disabled: true, action: null }
  }
  if (downloadAction.isLoading.value) {
    return { label: 'Скачиваю Zapret…', loading: true, disabled: true, action: null }
  }
  if (!bundle.value) {
    return { label: 'Установить Zapret', icon: Download, action: installAndEnable }
  }
  if (!installed.value) {
    return { label: 'Включить обход DPI', icon: ShieldCheck, action: enable }
  }
  if (running.value) {
    return { label: 'Выключить', icon: Square, variant: 'outline', action: stop }
  }
  return { label: 'Включить', icon: ShieldCheck, action: start }
})

async function togglePrimary() {
  if (primary.value.action) await primary.value.action()
}

async function installAndEnable() {
  try {
    await downloadAction.mutateAsync('zapret2')
    notify('Zapret установлен')
  } catch {
    return
  }
  await enableAfterDownload()
}

async function enableAfterDownload() {
  if (z.value.blockcheck) {
    await autoTune()
  } else {
    await installDefaultPreset()
  }
}

async function enable() {
  if (autoRunning.value) return
  if (!bundle.value) {
    await installAndEnable()
    return
  }
  if (installed.value) {
    if (running.value) {
      await stop()
    } else {
      await start()
    }
    return
  }
  if (z.value.blockcheck) {
    await autoTune()
  } else {
    await installDefaultPreset()
  }
}

async function start() {
  try {
    await startAction.mutateAsync()
    notify('Обход DPI включён')
  } catch {
    /* тост показан */
  }
}

async function stop() {
  try {
    await stopAction.mutateAsync()
    notify('Обход DPI выключен')
  } catch {
    /* тост показан */
  }
}

async function autoTune() {
  try {
    await autoAction.mutateAsync()
    notify('Ищу стратегию. Подтвердите UAC — обычно это занимает 1–3 минуты.')
  } catch {
    /* тост показан */
  }
}

async function installDefaultPreset() {
  const preset = manualPreset.value || z.value.preset || presetOptions.value[0]?.value
  if (!preset) {
    notify('В Zapret не найден ни один пресет', 'error')
    return
  }
  try {
    await installAction.mutateAsync([preset])
    notify('Установка службы запущена — подтвердите UAC')
  } catch {
    /* тост показан */
  }
}

async function probe() {
  try {
    probeResult.value = await probeAction.mutateAsync()
    const targets = probeResult.value?.targets || []
    const ok = targets.filter((item) => item.code !== '000').length
    notify(`Проверка: ${ok}/${targets.length} доменов отвечают`)
  } catch {
    /* тост показан */
  }
}

async function reinstallBundle() {
  if (!confirm('Скачать Zapret заново? Служба будет остановлена.')) return
  try {
    await downloadAction.mutateAsync('zapret2')
    notify('Zapret переустановлен')
  } catch {
    /* тост показан */
  }
}

async function removeService() {
  if (!confirm('Удалить службу Zapret?')) return
  try {
    await removeAction.mutateAsync()
    notify('Удаление службы запущено — подтвердите UAC')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Обход DPI"
    description="Одна кнопка: установить, подобрать стратегию и включить. Всё сложное спрятано в «Дополнительно»."
  >
    <template #icon>
      <ShieldAlert class="h-4 w-4 text-primary" />
    </template>

    <div v-if="!supported" class="rounded-lg border bg-muted/30 px-3 py-3 text-sm">
      Zapret использует WinDivert и службу Windows, поэтому на macOS он недоступен.
    </div>

    <div v-else class="space-y-4">
      <div
        class="rounded-xl border p-5"
        :class="running ? 'border-success/40 bg-success/5' : 'border-border bg-muted/20'"
      >
        <div class="flex items-start gap-3">
          <div
            class="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-full"
            :class="running ? 'bg-success/15 text-success' : 'bg-secondary text-muted-foreground'"
          >
            <ShieldCheck v-if="running" class="h-5 w-5" />
            <ShieldAlert v-else class="h-5 w-5" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <h3 class="text-lg font-semibold">{{ statusTitle }}</h3>
              <Badge v-if="running" variant="success">служба работает</Badge>
              <Badge v-else-if="installed" variant="secondary">служба установлена</Badge>
              <Badge v-else variant="outline">не настроено</Badge>
            </div>
            <p class="mt-1 text-sm text-muted-foreground">{{ statusText }}</p>

            <div class="mt-4 flex flex-wrap items-center gap-2">
              <Button
                data-testid="zapret-primary"
                size="lg"
                :variant="primary.variant || 'default'"
                :loading="primary.loading"
                :disabled="primary.disabled || busy"
                @click="togglePrimary"
              >
                <component :is="primary.icon" v-if="primary.icon" class="h-4 w-4" />
                {{ primary.label }}
              </Button>
              <Button
                v-if="running"
                size="sm"
                variant="ghost"
                :loading="probeAction.isLoading.value"
                :disabled="busy"
                @click="probe"
              >
                <FlaskConical class="h-3.5 w-3.5" />
                Проверить
              </Button>
            </div>

            <p v-if="!installed && !autoRunning" class="mt-2 text-xs text-muted-foreground">
              Первый запуск: скачивание Zapret, поиск стратегии и установка службы.
              Понадобится подтвердить UAC.
            </p>
          </div>
        </div>

        <div v-if="autoRunning" class="mt-4 rounded-lg border bg-background/60 px-3 py-2 text-xs">
          <div class="flex items-center gap-2 font-medium">
            <RotateCw class="h-3.5 w-3.5 animate-spin" />
            {{ phaseLabel }}
          </div>
          <p class="mt-1 text-muted-foreground">
            Проверяю домены: {{ (domains.length ? domains : ['www.youtube.com', 'discord.com']).join(', ') }}.
          </p>
        </div>

        <p v-if="z.error" class="mt-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {{ z.error }}
        </p>
        <p v-if="auto.error" class="mt-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          Не удалось подобрать стратегию. Нажмите «Включить» ещё раз или откройте
          «Дополнительно» → «Логи».
        </p>
      </div>

      <div
        v-if="z.strategyStale && !autoRunning"
        class="flex flex-wrap items-center gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3"
      >
        <RotateCw class="h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
        <div class="min-w-0 flex-1">
          <div class="text-sm font-medium">Список доменов изменился</div>
          <p class="mt-0.5 text-xs text-muted-foreground">
            Стратегия собрана для старого списка. Пересоберите её, чтобы учесть новые домены.
          </p>
        </div>
        <Button size="sm" variant="outline" :disabled="busy" @click="autoTune">
          Пересобрать стратегию
        </Button>
      </div>

      <div class="rounded-xl border p-4">
        <div class="flex flex-wrap items-start justify-between gap-2">
          <div>
            <div class="text-sm font-medium">Домены для обхода</div>
            <p class="mt-1 text-xs text-muted-foreground">
              Берутся из общего списка доменов. Для поиска стратегии используются
              первые 3 цели, остальные можно проверить точечно.
            </p>
          </div>
          <Badge variant="outline">{{ domains.length }}</Badge>
        </div>

        <div v-if="domains.length" class="mt-3 flex flex-wrap gap-1.5">
          <span
            v-for="domain in domains"
            :key="domain"
            class="inline-flex items-center rounded-md border bg-muted/40 px-2 py-1 text-xs"
          >
            {{ domain }}
          </span>
        </div>
        <p v-else class="mt-3 text-xs text-muted-foreground">
          Список пуст — будут использованы YouTube и Discord.
        </p>

        <RouterLink
          to="/routing"
          class="mt-3 inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
        >
          Изменить в общем списке
        </RouterLink>
      </div>

      <details class="rounded-xl border bg-muted/20 px-4 py-3">
        <summary class="cursor-pointer text-sm font-medium">Дополнительно</summary>
        <div class="mt-4 space-y-4">
          <div class="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" :disabled="busy || autoRunning" @click="autoTune">
              <RotateCw class="h-3.5 w-3.5" />
              Найти стратегию заново
            </Button>
            <Button size="sm" variant="outline" :disabled="busy || !running" @click="probe">
              <FlaskConical class="h-3.5 w-3.5" />
              Проверить результат
            </Button>
            <RouterLink
              to="/dpi/catalog"
              class="inline-flex h-8 items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
            >
              <ExternalLink class="h-3.5 w-3.5" />
              Каталог стратегий
            </RouterLink>
          </div>

          <div v-if="probeResult?.targets?.length" class="flex flex-wrap gap-1.5 text-xs">
            <Badge
              v-for="item in probeResult.targets"
              :key="item.name"
              :variant="item.code !== '000' ? 'success' : 'destructive'"
            >
              {{ item.name }}: {{ item.code === '000' ? 'нет ответа' : item.code }} · {{ item.ms }} мс
            </Badge>
          </div>

          <div class="flex flex-wrap items-end gap-2">
            <div class="min-w-56 flex-1 space-y-1">
              <div class="text-xs font-medium text-muted-foreground">Стандартный пресет</div>
              <Select v-model="manualPreset" :options="presetOptions" />
            </div>
            <Button size="sm" variant="outline" :loading="installAction.isLoading.value" :disabled="busy" @click="installDefaultPreset">
              Установить пресет
            </Button>
          </div>

          <div class="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" :loading="downloadAction.isLoading.value" :disabled="busy" @click="reinstallBundle">
              <Download class="h-3.5 w-3.5" />
              Переустановить Zapret
            </Button>
            <Button size="sm" variant="outline" :loading="openAction.isLoading.value" :disabled="busy" @click="openAction.mutateAsync()">
              <FolderOpen class="h-3.5 w-3.5" />
              Папка Zapret
            </Button>
            <Button size="sm" variant="destructive" :loading="removeAction.isLoading.value" :disabled="busy" @click="removeService">
              <Trash2 class="h-3.5 w-3.5" />
              Удалить службу
            </Button>
          </div>

          <details v-if="auto.log || auto.error || service.log" class="rounded-lg border bg-background/60 px-3 py-2 text-xs">
            <summary class="cursor-pointer text-muted-foreground">Логи</summary>
            <pre v-if="auto.error" class="mt-2 whitespace-pre-wrap break-all text-destructive">{{ auto.error }}</pre>
            <pre v-if="auto.log" class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all">{{ auto.log }}</pre>
            <pre v-if="service.log" class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all">{{ service.log }}</pre>
          </details>
        </div>
      </details>
    </div>
  </Card>
</template>
