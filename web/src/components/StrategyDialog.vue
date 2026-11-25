<script setup>
import { computed, ref, watch } from 'vue'
import { RefreshCw, Save, Wand2 } from 'lucide-vue-next'
import { api, notify, refresh } from '@/lib/store'
import Dialog from '@/components/ui/Dialog.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Select from '@/components/ui/Select.vue'

const props = defineProps({
  open: { type: Boolean, default: false },
  domain: { type: String, default: '' },
  s: { type: Object, required: true },
})
const emit = defineEmits(['close'])

const z = computed(() => props.s.zapret || {})
const auto = computed(() => z.value.autoTune || {})
const canGenerate = computed(
  () =>
    !!props.s.plugins?.zapret &&
    !!z.value.supported &&
    !!z.value.bundleInstalled &&
    !!z.value.blockcheck,
)
const running = computed(() => !!auto.value.running)
const thisDomain = computed(() => auto.value.domain === props.domain)
const candidates = computed(() => (thisDomain.value ? auto.value.candidates || [] : []))
const scan = ref('veryquick')
const scanOptions = [
  { value: 'veryquick', label: 'Очень быстро (рекомендуется)' },
  { value: 'quick', label: 'Быстро' },
  { value: 'standard', label: 'Стандартно' },
  { value: 'force', label: 'Полно (долго)' },
]
const busy = ref(false)
const actionBusy = ref('')
const localError = ref('')

const PHASES = {
  starting: 'Подготовка…',
  blockcheck: 'Перебираю стратегии…',
  installing: 'Устанавливаю стратегию…',
  done: 'Готово',
  error: 'Ошибка',
}

const phaseText = computed(() => PHASES[auto.value.phase] || auto.value.phase || '')
const errorText = computed(() => localError.value || (thisDomain.value ? auto.value.error : ''))

watch(
  () => props.open,
  (open) => {
    if (!open) return
    localError.value = ''
    if (!canGenerate.value || running.value) return
    if (thisDomain.value && candidates.value.length) return
    generate()
  },
  { immediate: true },
)

async function generate() {
  if (!props.domain || busy.value || running.value) return
  busy.value = true
  localError.value = ''
  try {
    await api('/api/zapret', {
      action: 'generate-strategies',
      domain: props.domain,
      scan: scan.value,
    })
    notify('Генерация стратегий запущена — подтвердите UAC')
    await refresh()
  } catch (error) {
    localError.value = error.message
    notify(error.message, 'error')
  } finally {
    busy.value = false
  }
}

async function applyCandidate(candidate) {
  actionBusy.value = `apply-${candidate.index}`
  try {
    await api('/api/zapret', { action: 'apply-strategy', index: candidate.index })
    notify('Установщик стратегии запущен — подтвердите UAC')
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    actionBusy.value = ''
  }
}

async function saveCandidate(candidate) {
  actionBusy.value = `save-${candidate.index}`
  try {
    const result = await api('/api/zapret', { action: 'save-strategy', index: candidate.index })
    notify(`Стратегия сохранена как пресет ${result.preset}`)
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    actionBusy.value = ''
  }
}
</script>

<template>
  <Dialog
    :open="open"
    wide
    :title="`Стратегии для ${domain}`"
    description="blockcheck2 перебирает DPI-стратегии для этого ресурса. Можно применить найденную или сохранить её в пресеты."
    @close="emit('close')"
  >
    <div class="space-y-4">
      <div
        v-if="!canGenerate"
        class="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400"
      >
        Генерация доступна на Windows с бандлом zapret2, в котором есть blockcheck2 и
        cygwin. Скачайте zapret2 в разделе «Обход DPI».
      </div>

      <template v-else>
        <div class="flex flex-wrap items-end gap-2">
          <div class="min-w-40 flex-1 space-y-1">
            <div class="text-xs font-medium text-muted-foreground">Глубина перебора</div>
            <Select v-model="scan" :options="scanOptions" :disabled="running || busy" />
          </div>
          <Button :loading="busy || running" :disabled="running || busy" @click="generate">
            <RefreshCw v-if="candidates.length" class="h-3.5 w-3.5" />
            <Wand2 v-else class="h-3.5 w-3.5" />
            {{ candidates.length ? 'Перегенерировать' : 'Сгенерировать' }}
          </Button>
        </div>

        <p class="text-xs text-muted-foreground">
          На время поиска служба Zapret останавливается, чтобы не мешать тестам, и
          запускается снова. Потребуется подтверждение UAC. Быстрый перебор обычно
          занимает 1–3 минуты, полный — дольше.
        </p>

        <div
          v-if="running"
          class="space-y-1 rounded-lg border bg-muted/30 px-3 py-2 text-xs"
        >
          <div class="flex items-center gap-2">
            <Badge variant="secondary">{{ phaseText }}</Badge>
            <span v-if="auto.domain && !thisDomain" class="text-muted-foreground">
              Сейчас идёт генерация для {{ auto.domain }}
            </span>
          </div>
          <pre
            v-if="auto.log"
            class="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border bg-background/60 p-2 text-[11px]"
          >{{ auto.log }}</pre>
        </div>

        <div v-if="errorText && !running" class="text-xs text-destructive">
          {{ errorText }}
        </div>

        <div v-if="!running && thisDomain && candidates.length" class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <div class="text-sm font-medium">Найдено стратегий: {{ candidates.length }}</div>
            <Badge variant="success">blockcheck2</Badge>
          </div>
          <div
            v-for="candidate in candidates"
            :key="candidate.index"
            class="space-y-2 rounded-lg border bg-background/60 px-3 py-2"
          >
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline">{{ candidate.label }}</Badge>
              <span class="text-[11px] text-muted-foreground">#{{ candidate.index + 1 }}</span>
            </div>
            <code class="block max-h-20 overflow-auto whitespace-pre-wrap break-all text-[11px] text-muted-foreground">{{ candidate.strategy }}</code>
            <div class="flex flex-wrap gap-2">
              <Button
                size="sm"
                :loading="actionBusy === `apply-${candidate.index}`"
                :disabled="!!actionBusy || running"
                @click="applyCandidate(candidate)"
              >
                Применить
              </Button>
              <Button
                size="sm"
                variant="outline"
                :loading="actionBusy === `save-${candidate.index}`"
                :disabled="!!actionBusy || running"
                @click="saveCandidate(candidate)"
              >
                <Save class="h-3.5 w-3.5" />
                В пресеты
              </Button>
            </div>
          </div>
          <p v-if="auto.preset" class="text-xs text-muted-foreground">
            Сохранено: <code>{{ auto.preset }}</code> — выберите его в разделе «Обход DPI»
            или объедините с другими пресетами.
          </p>
        </div>
      </template>
    </div>

    <template #footer>
      <Button variant="outline" @click="emit('close')">Закрыть</Button>
    </template>
  </Dialog>
</template>
