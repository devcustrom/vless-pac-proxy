<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { marked } from 'marked'
import { BookOpen, X } from 'lucide-vue-next'
import Badge from './ui/Badge.vue'
import Button from './ui/Button.vue'

const props = defineProps({ open: { type: Boolean, default: false } })
const emit = defineEmits(['close'])

const docs = ref(null)
const loading = ref(false)
const error = ref('')
const tab = ref('readme')

const html = computed(() => {
  const text = tab.value === 'readme'
    ? docs.value?.readme
    : tab.value === 'mac'
      ? docs.value?.readmeMac
      : docs.value?.plugins
  if (!text) return ''
  return marked.parse(text, { mangle: false, headerIds: false })
})

async function load() {
  if (docs.value || loading.value) return
  loading.value = true
  try {
    const response = await fetch('/api/docs')
    const data = await response.json()
    if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`)
    docs.value = data
  } catch (e) {
    error.value = e.message
  } finally {
    loading.value = false
  }
}

watch(() => props.open, (open) => {
  if (open) load()
})

function onKey(event) {
  if (event.key === 'Escape') emit('close')
}

onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-3 sm:p-6"
      @click.self="emit('close')"
    >
      <div class="flex max-h-[90vh] w-full max-w-4xl flex-col overflow-hidden rounded-2xl border bg-card shadow-2xl">
        <div class="flex items-center gap-3 border-b px-5 py-3">
          <BookOpen class="h-5 w-5 text-primary" />
          <h2 class="text-base font-semibold">Документация</h2>
          <Badge variant="outline">v{{ docs?.version || '…' }}</Badge>
          <Button variant="ghost" size="icon" class="ml-auto" @click="emit('close')">
            <X class="h-4 w-4" />
          </Button>
        </div>

        <div class="flex gap-1 border-b px-4 py-2">
          <Button
            size="sm"
            :variant="tab === 'readme' ? 'secondary' : 'ghost'"
            @click="tab = 'readme'"
          >
            Windows / общая
          </Button>
          <Button
            size="sm"
            :variant="tab === 'mac' ? 'secondary' : 'ghost'"
            @click="tab = 'mac'"
          >
            macOS
          </Button>
          <Button
            size="sm"
            :variant="tab === 'plugins' ? 'secondary' : 'ghost'"
            @click="tab = 'plugins'"
          >
            Плагины и протоколы
          </Button>
        </div>

        <div class="min-h-0 flex-1 overflow-auto px-5 py-4">
          <p v-if="loading" class="text-sm text-muted-foreground">Загружаю документацию…</p>
          <p v-else-if="error" class="text-sm text-destructive">{{ error }}</p>
          <div v-else class="markdown" v-html="html" />
        </div>
      </div>
    </div>
  </teleport>
</template>
