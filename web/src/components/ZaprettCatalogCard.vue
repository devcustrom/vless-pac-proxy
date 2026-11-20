<script setup>
import { computed, ref, watch } from 'vue'
import { Download, ExternalLink, Info, RefreshCw } from 'lucide-vue-next'
import { api, notify, refresh } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'

const props = defineProps({ s: { type: Object, required: true } })

const cat = computed(() => props.s.zapret?.catalog || {})
const tab = ref('strategies')
const query = ref('')
const busy = ref('')
const refreshReport = ref(null)
const selectionDirty = ref(false)
const selectedLists = ref([])
const selectedIpsets = ref([])
const testDomains = ref('www.youtube.com, discord.com')
const testBusy = ref(false)
const test = computed(() => cat.value.test || null)
const testResults = computed(() =>
  [...(test.value?.results || [])].sort(
    (left, right) => right.reachable.length - left.reachable.length,
  ),
)

watch(
  () => cat.value.selectedLists,
  (value) => {
    if (!selectionDirty.value) selectedLists.value = [...(value || [])]
  },
  { immediate: true, deep: true },
)
watch(
  () => cat.value.selectedIpsets,
  (value) => {
    if (!selectionDirty.value) selectedIpsets.value = [...(value || [])]
  },
  { immediate: true, deep: true },
)

const TABS = [
  { value: 'strategies', label: 'Стратегии', key: 'strategies' },
  { value: 'lists', label: 'Списки', key: 'lists' },
  { value: 'ipsets', label: 'Ipsets', key: 'ipsets' },
  { value: 'bins', label: 'Бинарники', key: 'bins' },
]

const groups = computed(() => ({
  strategies: cat.value.strategies || [],
  lists: cat.value.lists || [],
  ipsets: cat.value.ipsets || [],
  bins: cat.value.bins || [],
}))

const current = computed(() => {
  const q = query.value.trim().toLowerCase()
  const items = groups.value[tab.value] || []
  if (!q) return items
  return items.filter((item) =>
    [item.id, item.name, item.description, item.author]
      .filter(Boolean)
      .some((value) => String(value).toLowerCase().includes(q)),
  )
})

const sourceLink = computed(() => cat.value.sourcePage || cat.value.source || '')
const updatedLabel = computed(() => {
  const value = cat.value.updatedAt
  if (!value) return 'ещё не обновлялся'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('ru-RU')
})
const busyAny = computed(() => !!busy.value)
const selectedCount = computed(() => selectedLists.value.length + selectedIpsets.value.length)

function kindLabel(kind) {
  if (kind === 'nfqws') return 'winws v1'
  if (kind === 'nfqws2') return 'winws2'
  if (kind === 'list') return 'hostlist'
  if (kind === 'list_exclude') return 'hostlist exclude'
  if (kind === 'ipset') return 'ipset'
  if (kind === 'ipset_exclude') return 'ipset exclude'
  if (kind === 'bin') return 'fake payload'
  if (kind === 'lua_lib') return 'lua'
  return kind
}

function canSelectInclude(item) {
  return ['list', 'ipset'].includes(item.kind) && item.downloaded
}

function isSelected(item) {
  return item.kind === 'list'
    ? selectedLists.value.includes(item.id)
    : selectedIpsets.value.includes(item.id)
}

function toggleInclude(item) {
  if (!canSelectInclude(item)) return
  const list = item.kind === 'list' ? selectedLists : selectedIpsets
  const index = list.value.indexOf(item.id)
  if (index >= 0) list.value.splice(index, 1)
  else list.value.push(item.id)
  selectionDirty.value = true
}

async function saveSelection(quiet = false) {
  busy.value = 'select'
  try {
    await api('/api/zapret', {
      action: 'catalog-select',
      lists: selectedLists.value,
      ipsets: selectedIpsets.value,
    })
    selectionDirty.value = false
    await refresh()
    if (!quiet) notify(`Выбрано списков: ${selectedCount.value}`)
  } catch (error) {
    if (!quiet) notify(error.message, 'error')
    throw error
  } finally {
    busy.value = ''
  }
}

async function refreshCatalog() {
  busy.value = 'refresh'
  try {
    const result = await api('/api/zapret', { action: 'catalog-refresh' })
    refreshReport.value = result.report || null
    await refresh()
    const report = result.report || {}
    notify(
      report.failed
        ? `Каталог обновлён: ${report.updated}, ошибок: ${report.failed}`
        : `Каталог обновлён: ${report.updated} элементов`,
    )
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = ''
  }
}

async function downloadItem(item) {
  busy.value = `download:${item.id}`
  try {
    const result = await api('/api/zapret', { action: 'catalog-download', id: item.id })
    notify(`Скачано файлов: ${result.downloaded || 1}`)
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = ''
  }
}

async function applyStrategy(item) {
  if (
    !confirm(
      `Применить стратегию «${item.name || item.id}»?\n\n` +
        'Она заменит текущую конфигурацию службы Zapret. Потребуется подтверждение UAC.',
    )
  ) {
    return
  }
  busy.value = `apply:${item.id}`
  try {
    if (selectionDirty.value) await saveSelection(true)
    await api('/api/zapret', { action: 'catalog-apply', id: item.id })
    notify('Установщик стратегии каталога запущен — подтвердите UAC')
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = ''
  }
}

function strategyFor(id) {
  return (cat.value.strategies || []).find((item) => item.id === id) || null
}

function applyResult(result) {
  const item = strategyFor(result.id)
  if (!item) {
    notify('Стратегия больше не найдена в каталоге', 'error')
    return
  }
  applyStrategy(item)
}

async function startTest() {
  const domains = testDomains.value
    .split(/[\s,;]+/)
    .map((value) => value.trim())
    .filter(Boolean)
  if (!domains.length) {
    notify('Укажи хотя бы один домен', 'error')
    return
  }
  testBusy.value = true
  try {
    await api('/api/zapret', { action: 'catalog-test', domains })
    notify('Проверка стратегий запущена — подтвердите UAC')
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    testBusy.value = false
  }
}

async function resetTest() {
  if (!confirm('Сбросить статус и результаты проверки стратегий?')) return
  testBusy.value = true
  try {
    await api('/api/zapret', { action: 'catalog-test-reset', force: true })
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    testBusy.value = false
  }
}

function codeVariant(code) {
  return code && code !== '000' ? 'success' : 'destructive'
}
</script>

<template>
  <Card
    title="Каталог Zaprett"
    description="Готовые стратегии, hostlists, ipsets и fake-payload'ы из внешнего репозитория. У каждого элемента указан автор и источник."
  >
    <template #icon>
      <Info class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="cat.cached ? 'success' : 'warning'">
          {{ cat.cached ? 'каталог загружен' : 'каталог не загружен' }}
        </Badge>
        <span class="text-muted-foreground">обновлён: {{ updatedLabel }}</span>
        <a
          v-if="sourceLink"
          :href="sourceLink"
          target="_blank"
          rel="noreferrer"
          class="inline-flex items-center gap-1 text-primary hover:underline"
        >
          <ExternalLink class="h-3 w-3" />
          {{ sourceLink }}
        </a>
        <Button
          size="sm"
          variant="outline"
          class="ml-auto"
          :loading="busy === 'refresh'"
          :disabled="busyAny"
          @click="refreshCatalog"
        >
          <RefreshCw class="h-3.5 w-3.5" />
          Обновить каталог
        </Button>
      </div>

      <p v-if="refreshReport?.failed" class="text-[11px] text-amber-600 dark:text-amber-400">
        Не удалось загрузить манифестов: {{ refreshReport.failed }}.
        {{ refreshReport.errors?.[0] }}
      </p>

      <div class="space-y-2 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2">
        <div class="text-sm font-medium">Проверка стратегий по домену</div>
        <div class="flex flex-wrap gap-2">
          <Input
            v-model="testDomains"
            class="min-w-56 flex-1"
            placeholder="www.youtube.com, discord.com"
            :disabled="!!test?.running || testBusy"
          />
          <Button
            size="sm"
            :loading="testBusy"
            :disabled="testBusy || !!test?.running || !cat.cached"
            @click="startTest"
          >
            Проверить скачанные
          </Button>
        </div>
        <p class="text-[11px] text-muted-foreground">
          Последовательно запускает скачанные winws/winws2-стратегии и проверяет домены через
          прямой HTTPS. Служба Zapret на время проверки останавливается и запускается снова.
          Нужен один UAC. Результаты сортируются по числу доступных доменов. Для осмысленного
          рейтинга берите домен, который без обхода не открывается — иначе все стратегии
          получат 100%.
        </p>
        <div v-if="test?.running" class="space-y-1 text-xs">
          <div class="flex flex-wrap items-center gap-2">
            <Badge variant="secondary">
              {{ test.done }} / {{ test.total }}
            </Badge>
            <span v-if="test.current" class="text-muted-foreground">
              сейчас: {{ test.current }}
            </span>
          </div>
          <div class="h-1.5 overflow-hidden rounded-full bg-muted">
            <div
              class="h-full bg-primary transition-all"
              :style="{ width: `${test.total ? (test.done / test.total) * 100 : 0}%` }"
            />
          </div>
        </div>
        <p v-if="test?.error" class="text-xs text-destructive">{{ test.error }}</p>

        <div v-if="test && !test.running && testResults.length" class="space-y-2">
          <div
            v-for="result in testResults"
            :key="result.id"
            class="space-y-1 rounded-md border bg-background/60 px-2 py-1.5"
          >
            <div class="flex flex-wrap items-center gap-2">
              <span class="min-w-0 flex-1 break-all text-xs font-medium">
                {{ result.name || result.id }}
              </span>
              <Badge
                :variant="
                  result.reachable.length === result.codes.length && result.codes.length
                    ? 'success'
                    : result.reachable.length
                      ? 'warning'
                      : 'destructive'
                "
              >
                {{ result.reachable.length }} / {{ result.codes.length }}
              </Badge>
              <Button
                size="sm"
                variant="outline"
                :disabled="!strategyFor(result.id) || !strategyFor(result.id)?.ready"
                :loading="busy === `apply:${result.id}`"
                @click="applyResult(result)"
              >
                Применить
              </Button>
            </div>
            <div class="flex flex-wrap gap-1">
              <Badge
                v-for="code in result.codes"
                :key="code.domain"
                :variant="codeVariant(code.code)"
              >
                {{ code.domain }}: {{ code.code === '000' ? 'нет' : code.code }} · {{ code.ms }} мс
              </Badge>
            </div>
            <p v-if="result.error" class="text-[11px] text-destructive">{{ result.error }}</p>
            <p class="text-[11px] text-muted-foreground">
              {{ result.kind === 'nfqws' ? 'winws v1' : 'winws2' }} · автор:
              {{ result.author || 'не указан' }}
            </p>
          </div>
          <Button size="sm" variant="ghost" :disabled="testBusy" @click="resetTest">
            Сбросить результаты
          </Button>
        </div>
        <Button
          v-else-if="test && !test.running"
          size="sm"
          variant="ghost"
          :disabled="testBusy"
          @click="resetTest"
        >
          Сбросить статус
        </Button>
      </div>

      <div class="flex flex-wrap gap-1">
        <Button
          v-for="item in TABS"
          :key="item.value"
          size="sm"
          :variant="tab === item.value ? 'secondary' : 'ghost'"
          @click="tab = item.value"
        >
          {{ item.label }}
          <span class="ml-1 text-xs text-muted-foreground">{{ (groups[item.key] || []).length }}</span>
        </Button>
      </div>

      <div class="flex gap-2">
        <Input v-model="query" placeholder="Поиск по названию, автору, описанию…" />
      </div>

      <div
        v-if="tab === 'lists' || tab === 'ipsets'"
        class="space-y-2 rounded-lg border bg-muted/30 px-3 py-2 text-xs"
      >
        <div class="flex flex-wrap items-center gap-2">
          <span class="font-medium">Выбрано для подстановки:</span>
          <Badge variant="outline">списков {{ selectedLists.length }}</Badge>
          <Badge variant="outline">ipsets {{ selectedIpsets.length }}</Badge>
          <Button
            size="sm"
            variant="outline"
            :loading="busy === 'select'"
            :disabled="busyAny || !selectionDirty"
            @click="saveSelection(false)"
          >
            Сохранить выбор
          </Button>
        </div>
        <p class="text-muted-foreground">
          Отмеченные include-списки подставляются вместо <code>${hostlists}</code>, include-ipsets —
          вместо <code>${ipsets}</code>. Exclude-элементы можно скачать, но в общий список они не
          входят. Если ничего не выбрано, эти переменные станут пустыми и профили стратегии будут
          применяться без фильтра по доменам/IP.
        </p>
      </div>

      <div v-if="!cat.cached" class="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-3 text-xs">
        Каталог ещё не загружен. Нажми «Обновить каталог» — будет прочитан
        <code>index.json</code> и манифесты. Артефакты скачиваются только по кнопке у элемента.
      </div>

      <div v-else class="max-h-[32rem] space-y-2 overflow-auto pr-1">
        <div
          v-for="item in current"
          :key="`${item.kind}:${item.id}`"
          class="space-y-2 rounded-lg border bg-background/60 px-3 py-2"
        >
          <div class="flex flex-wrap items-start gap-2">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="break-all text-sm font-medium">{{ item.name || item.id }}</span>
                <Badge v-if="item.active" variant="success">активна</Badge>
                <Badge v-else-if="item.downloaded" variant="secondary">скачана</Badge>
                <Badge v-if="item.kind.startsWith('nfq') && !item.supported" variant="destructive">
                  нужен движок
                </Badge>
              </div>
              <p v-if="item.description" class="mt-0.5 text-xs text-muted-foreground">
                {{ item.description }}
              </p>
              <p class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
                <span>Автор: <b>{{ item.author || 'не указан' }}</b></span>
                <span>·</span>
                <span>{{ kindLabel(item.kind) }}</span>
                <span>·</span>
                <span>v{{ item.version || '?' }}</span>
                <span v-if="item.dependencies">· зависимостей: {{ item.dependencies }}</span>
                <a
                  v-if="item.source"
                  :href="item.source"
                  target="_blank"
                  rel="noreferrer"
                  class="inline-flex items-center gap-1 text-primary hover:underline"
                >
                  <ExternalLink class="h-3 w-3" />
                  манифест
                </a>
              </p>
            </div>
          </div>

          <div v-if="tab === 'strategies'" class="flex flex-wrap items-center gap-2">
            <Button
              v-if="!item.downloaded"
              size="sm"
              variant="outline"
              :loading="busy === `download:${item.id}`"
              :disabled="busyAny"
              @click="downloadItem(item)"
            >
              <Download class="h-3.5 w-3.5" />
              Скачать
            </Button>
            <Button
              v-else
              size="sm"
              :loading="busy === `apply:${item.id}`"
              :disabled="busyAny || !item.ready || !item.supported"
              @click="applyStrategy(item)"
            >
              Применить
            </Button>
            <Button
              v-if="item.downloaded"
              size="sm"
              variant="ghost"
              :loading="busy === `download:${item.id}`"
              :disabled="busyAny"
              @click="downloadItem(item)"
            >
              Обновить
            </Button>
            <span v-if="!item.supported && item.kind !== 'byedpi'" class="text-[11px] text-destructive">
              В бандле нет подходящего winws/winws2
            </span>
          </div>

          <div v-else class="flex flex-wrap items-center gap-2">
            <label
              v-if="item.kind === 'list' || item.kind === 'ipset'"
              class="flex items-center gap-2 text-xs"
              :class="canSelectInclude(item) ? 'cursor-pointer' : 'text-muted-foreground'"
            >
              <input
                type="checkbox"
                class="h-4 w-4 accent-primary"
                :disabled="!canSelectInclude(item) || busyAny"
                :checked="isSelected(item)"
                @change="toggleInclude(item)"
              />
              использовать в {{ item.kind === 'list' ? 'hostlists' : 'ipsets' }}
            </label>
            <Button
              v-if="!item.downloaded"
              size="sm"
              variant="outline"
              :loading="busy === `download:${item.id}`"
              :disabled="busyAny"
              @click="downloadItem(item)"
            >
              <Download class="h-3.5 w-3.5" />
              Скачать
            </Button>
            <Badge v-else variant="secondary">скачано</Badge>
          </div>
        </div>
        <p v-if="!current.length" class="text-xs text-muted-foreground">
          Ничего не найдено.
        </p>
      </div>

      <div class="rounded-lg border bg-muted/30 px-3 py-2 text-[11px] text-muted-foreground">
        {{ cat.licenseNote || 'Данные загружаются из внешнего репозитория; указывайте источник.' }}
      </div>
    </div>
  </Card>
</template>
