<script setup>
import { computed, reactive, ref, watch } from 'vue'
import { Download, FolderOpen, Play, ShieldAlert, Square, Trash2 } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Select from '@/components/ui/Select.vue'

const props = defineProps({ s: { type: Object, required: true } })

const z = computed(() => props.s.zapret || {})
const enabled = computed(() => !!props.s.plugins?.zapret)
const supported = computed(() => z.value.supported !== false)
const bundle = computed(() => !!z.value.bundleInstalled)
const service = computed(() => z.value.service || {})
const running = computed(() => !!service.value.running)
const installed = computed(() => !!service.value.installed)
const form = reactive({ preset: 'general.bat' })
const saved = ref('')
const dirty = ref(false)

const presetOptions = computed(() =>
  (z.value.presets || []).map((item) => ({ value: item.name, label: item.label })),
)

function syncForm() {
  const current = z.value.preset || 'general.bat'
  saved.value = current
  if (!dirty.value) form.preset = current
}

watch(z, syncForm, { immediate: true, deep: true })

const installAction = useActionMutation(
  () => api('/api/zapret', { action: 'install', preset: form.preset }),
  [['status']],
)
const downloadAction = useActionMutation(
  (variant) => api('/api/zapret', { action: 'download', variant }),
  [['status']],
)
const autoAction = useActionMutation(
  () => api('/api/zapret', { action: 'auto-tune' }),
  [['status']],
)
const poolAutoAction = useActionMutation(
  () => api('/api/pool/auto', {}),
  [['status']],
)
const probeAction = useActionMutation(
  () => api('/api/zapret', { action: 'probe' }),
  [['status']],
)
const probeResult = ref(null)
const removeAction = useActionMutation(
  () => api('/api/zapret', { action: 'remove' }),
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
const openAction = useActionMutation(
  () => api('/api/zapret', { action: 'open-folder' }),
  [['status']],
)
const pluginAction = useActionMutation(
  (value) => api('/api/plugins', { zapret: value }),
  [['status']],
)

const busy = computed(
  () =>
    installAction.isLoading.value ||
    downloadAction.isLoading.value ||
    autoAction.isLoading.value ||
    poolAutoAction.isLoading.value ||
    probeAction.isLoading.value ||
    removeAction.isLoading.value ||
    startAction.isLoading.value ||
    stopAction.isLoading.value ||
    pluginAction.isLoading.value,
)
const auto = computed(() => z.value.autoTune || {})
const autoRunning = computed(() => !!auto.value.running || !!autoAction.isLoading.value)
const autoAllRunning = computed(() => poolAutoAction.isLoading.value || autoAction.isLoading.value)
const openBusy = computed(() => openAction.isLoading.value)
const downloading = computed(() => downloadAction.isLoading.value)
const bundleInfo = computed(() => z.value.bundle || {})
const bundleLabel = computed(() => {
  if (bundleInfo.value.variant === 'flowseal') return 'Flowseal v1'
  if (bundleInfo.value.variant === 'zapret2') return 'zapret2'
  return z.value.engine && z.value.engine !== 'none' ? z.value.engine : 'Zapret'
})

async function install() {
  try {
    await installAction.mutateAsync()
    dirty.value = false
    notify('Установщик службы Zapret запущен — подтвердите UAC')
  } catch {
    /* тост показан */
  }
}

async function downloadBundle(variant) {
  const label = variant === 'flowseal' ? 'Flowseal v1' : 'zapret2'
  if (
    bundle.value &&
    !confirm(
      `Скачать и заменить папку Zapret на ${label}? Служба на время замены остановится.`,
    )
  ) {
    return
  }
  notify(`Скачиваю ${label} — это может занять до минуты`)
  try {
    await downloadAction.mutateAsync(variant)
    dirty.value = false
    notify(
      installed.value
        ? `Бандл ${label} установлен — проверьте пресет и нажмите «Применить пресет»`
        : `Бандл ${label} установлен`,
    )
  } catch {
    /* тост показан */
  }
}

async function autoTune() {
  try {
    await autoAction.mutateAsync()
    notify('Автоподбор стратегии запущен — подтвердите UAC. Это может занять несколько минут.')
  } catch {
    /* тост показан */
  }
}

async function autoAll() {
  let provider = ''
  try {
    const result = await poolAutoAction.mutateAsync()
    provider = result?.info?.name ? `Провайдер: ${result.info.name}. ` : ''
  } catch {
    provider = 'Провайдер: нет проверенных конфигов. '
  }
  if (z.value.blockcheck) {
    notify(provider + 'Запускаю подбор стратегии DPI — подтвердите UAC.')
    await autoTune()
  } else {
    notify(provider + 'Для автоподбора стратегии нужен zapret2 с blockcheck2.', 'error')
  }
}

async function probeStrategy() {
  try {
    const result = await probeAction.mutateAsync()
    probeResult.value = result
    if (!result.running) {
      notify(result.error || 'Служба Zapret не запущена', 'error')
      return
    }
    const ok = (result.targets || []).filter((item) => item.code !== '000').length
    notify(`Стратегия проверена: ${ok}/${(result.targets || []).length} целей отвечают`)
  } catch {
    /* тост показан */
  }
}

async function remove() {
  if (!confirm('Удалить службу Zapret? Бандл и списки останутся.')) return
  try {
    await removeAction.mutateAsync()
    notify('Удаление службы запущено — подтвердите UAC')
  } catch {
    /* тост показан */
  }
}

async function start() {
  try {
    await startAction.mutateAsync()
    notify('Служба Zapret запускается')
  } catch {
    /* тост показан */
  }
}

async function stop() {
  try {
    await stopAction.mutateAsync()
    notify('Служба Zapret останавливается')
  } catch {
    /* тост показан */
  }
}

async function openFolder() {
  try {
    await openAction.mutateAsync()
  } catch {
    /* тост показан */
  }
}

async function togglePlugin(value) {
  try {
    await pluginAction.mutateAsync(value)
    notify(value ? 'Плагин Zapret включён' : 'Плагин Zapret выключен')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Обход DPI"
    description="Zapret (zapret2 и Flowseal v1) как методы обхода DPI. Сюда же можно добавлять другие проекты."
  >
    <template #icon>
      <ShieldAlert class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="supported ? 'outline' : 'destructive'">
          {{ supported ? 'Windows' : 'только Windows' }}
        </Badge>
        <Badge :variant="enabled ? 'success' : 'secondary'">
          {{ enabled ? 'плагин включён' : 'плагин выключен' }}
        </Badge>
        <Badge :variant="bundle ? 'success' : 'warning'">
          {{ bundle ? 'бандл найден' : 'бандл не найден' }}
        </Badge>
        <Badge :variant="running ? 'success' : installed ? 'secondary' : 'outline'">
          {{ running ? 'служба работает' : installed ? 'служба остановлена' : 'служба не установлена' }}
        </Badge>
        <Badge v-if="z.engine && z.engine !== 'none'" variant="outline">
          {{ z.engine }}
        </Badge>
        <Badge v-if="bundleInfo.variant" variant="outline">
          {{ bundleLabel }}<template v-if="bundleInfo.version"> · {{ bundleInfo.version }}</template>
        </Badge>
      </div>

      <div class="flex flex-wrap items-center gap-2 text-xs">
        <span class="text-muted-foreground">Методы обхода DPI:</span>
        <Badge :variant="z.engine === 'winws2' ? 'success' : 'outline'">Zapret 2 (winws2)</Badge>
        <Badge :variant="z.engine === 'winws' ? 'success' : 'outline'">Flowseal v1 (winws)</Badge>
        <span class="text-muted-foreground">другие проекты можно добавить сюда же</span>
      </div>

      <div
        v-if="supported && bundle"
        class="space-y-2 rounded-lg border bg-muted/30 px-3 py-3"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div>
            <div class="text-sm font-medium">Проверка стратегии</div>
            <p class="text-xs text-muted-foreground">
              Проверит YouTube и Discord через запущенную службу Zapret.
              Полный перебор стратегий — blockcheck2.
            </p>
          </div>
          <div class="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              :loading="probeAction.isLoading.value"
              :disabled="busy"
              @click="probeStrategy"
            >
              Проверить стратегию
            </Button>
            <Button
              v-if="z.blockcheck"
              size="sm"
              :loading="autoRunning"
              :disabled="busy"
              @click="autoTune"
            >
              Перебрать стратегии
            </Button>
          </div>
        </div>
        <div v-if="probeResult" class="space-y-1 text-xs">
          <div v-if="!probeResult.running" class="text-destructive">
            {{ probeResult.error }}
          </div>
          <div v-else class="flex flex-wrap gap-1.5">
            <Badge
              v-for="item in probeResult.targets"
              :key="item.name"
              :variant="item.code !== '000' ? 'success' : 'destructive'"
            >
              {{ item.name }}: {{ item.code === '000' ? 'нет ответа' : item.code }} · {{ item.ms }} мс
            </Badge>
          </div>
        </div>
      </div>

      <div v-if="!supported" class="rounded-lg border bg-muted/30 px-3 py-2 text-sm">
        Zapret использует WinDivert и Windows-службу, поэтому на macOS он недоступен.
      </div>

      <div
        v-if="supported && !bundle"
        class="space-y-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400"
      >
        <p>
          Бандл не найден. Нажмите кнопку — приложение само скачает архив с
          GitHub, проверит sha256 и распакует его в
          <code class="break-all">{{ z.bundlePath }}</code>.
        </p>
        <p>
          <b>zapret2</b> (~20 МБ) — рекомендуется: <code>winws2</code>, Lua,
          blockcheck2 и cygwin. <b>Flowseal v1</b> (~1.5 МБ) — простые
          <code>general*.bat</code>.
        </p>
        <div class="flex flex-wrap gap-2">
          <Button size="sm" :loading="downloading" :disabled="busy" @click="downloadBundle('zapret2')">
            <Download class="h-3.5 w-3.5" />
            Скачать zapret2
          </Button>
          <Button
            size="sm"
            variant="outline"
            :loading="downloading"
            :disabled="busy"
            @click="downloadBundle('flowseal')"
          >
            <Download class="h-3.5 w-3.5" />
            Скачать Flowseal v1
          </Button>
          <Button size="sm" variant="outline" :loading="openBusy" @click="openFolder">
            <FolderOpen class="h-3.5 w-3.5" />
            Папка Zapret
          </Button>
        </div>
      </div>

      <div v-if="supported && bundle" class="space-y-3">
        <div class="flex flex-wrap items-center justify-between gap-2 rounded-lg border bg-muted/30 px-3 py-2 text-xs">
          <span>
            Бандл: <b>{{ bundleLabel }}</b>
            <span v-if="bundleInfo.version" class="text-muted-foreground"> · {{ bundleInfo.version }}</span>
          </span>
          <div class="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" :loading="downloading" :disabled="busy" @click="downloadBundle('zapret2')">
              <Download class="h-3.5 w-3.5" />
              Переустановить zapret2
            </Button>
            <Button
              size="sm"
              variant="ghost"
              :loading="downloading"
              :disabled="busy"
              @click="downloadBundle('flowseal')"
            >
              Flowseal v1
            </Button>
          </div>
        </div>
        <div
          v-if="z.engine === 'winws2' && z.blockcheck"
          class="space-y-3 rounded-lg border border-primary/30 bg-primary/5 p-3"
        >
          <div>
            <div class="text-sm font-medium">Автоматический подбор</div>
            <p class="text-xs text-muted-foreground">
              Выберет самый быстрый рабочий конфиг из пула, затем запустит
              blockcheck2 и установит рабочую стратегию DPI. Стратегии вручную
              добавлять не нужно.
            </p>
          </div>
          <div class="flex flex-wrap gap-2">
            <Button
              size="sm"
              :loading="autoAllRunning"
              :disabled="busy"
              @click="autoAll"
            >
              Подобрать провайдера и стратегию
            </Button>
            <Button
              size="sm"
              variant="outline"
              :loading="autoRunning"
              :disabled="busy"
              @click="autoTune"
            >
              Только стратегию
            </Button>
          </div>
          <p v-if="auto.phase" class="text-xs text-muted-foreground">
            Этап: {{ auto.phase }}
            <span v-if="auto.error" class="text-destructive"> · {{ auto.error }}</span>
          </p>
          <pre
            v-if="auto.log"
            class="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border bg-background/60 p-2 text-[11px]"
          >{{ auto.log }}</pre>
          <p v-if="auto.strategy" class="break-all text-[11px] text-muted-foreground">
            Выбрана: <code>{{ auto.strategy }}</code>
          </p>
        </div>

        <div class="space-y-1">
          <div class="text-sm font-medium">Или выбрать пресет вручную</div>
          <Select
            v-model="form.preset"
            :options="presetOptions"
            :disabled="busy || !presetOptions.length"
            @update:model-value="dirty = true"
          />
          <p class="text-xs text-muted-foreground">
            Выбранный пресет применяется к службе. При смене пресета нажмите
            «Установить / применить».
          </p>
        </div>

        <div class="flex flex-wrap gap-2">
          <Button size="sm" :loading="installAction.isLoading.value" :disabled="busy" @click="install">
            {{ installed ? 'Применить пресет' : 'Установить службу' }}
          </Button>
          <Button
            v-if="installed && !running"
            size="sm"
            variant="outline"
            :loading="startAction.isLoading.value"
            :disabled="busy"
            @click="start"
          >
            <Play class="h-3.5 w-3.5" />
            Запустить
          </Button>
          <Button
            v-if="running"
            size="sm"
            variant="outline"
            :loading="stopAction.isLoading.value"
            :disabled="busy"
            @click="stop"
          >
            <Square class="h-3.5 w-3.5" />
            Остановить
          </Button>
          <Button
            v-if="installed"
            size="sm"
            variant="ghost"
            :loading="removeAction.isLoading.value"
            :disabled="busy"
            @click="remove"
          >
            <Trash2 class="h-3.5 w-3.5" />
            Удалить службу
          </Button>
          <Button size="sm" variant="ghost" :loading="openBusy" @click="openFolder">
            <FolderOpen class="h-3.5 w-3.5" />
            Папка
          </Button>
        </div>

        <p v-if="dirty && form.preset !== saved" class="text-xs text-amber-600 dark:text-amber-400">
          Пресет изменён, но служба ещё работает со старым — нажмите «Применить пресет».
        </p>
      </div>

      <div v-if="supported && !enabled" class="flex items-center justify-between gap-3 rounded-lg border px-3 py-2">
        <span class="text-xs text-muted-foreground">
          Плагин выключен. Служба Zapret при этом может продолжать работать.
        </span>
        <Button size="sm" variant="outline" :loading="pluginAction.isLoading.value" @click="togglePlugin(true)">
          Включить плагин
        </Button>
      </div>

      <div v-if="z.error" class="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
        {{ z.error }}
      </div>

      <div v-if="supported" class="rounded-lg border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        <p>
          Для установки и удаления службы нужен UAC. Запуск и остановка после
          установки идут без повторного запроса. WinDivert может вызывать
          срабатывания антивируса — добавьте папку Zapret в исключения.
        </p>
        <p class="mt-1">
          Режим «Сначала напрямую» в карточке каналов сначала проверяет прямой
          путь через Zapret и только потом отправляет домен в VPN.
        </p>
      </div>

      <details v-if="service.log" class="rounded-lg border bg-muted/30 px-3 py-2 text-xs">
        <summary class="cursor-pointer text-muted-foreground">Лог службы</summary>
        <pre class="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all">{{ service.log }}</pre>
      </details>
    </div>
  </Card>
</template>
