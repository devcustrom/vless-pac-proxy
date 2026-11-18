<script setup>
import { computed, ref } from 'vue'
import { Download, LockKeyhole, Wifi } from 'lucide-vue-next'
import { ensureEngines, installCerts, post } from '@/lib/store'
import { useStatusQuery } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'
import Badge from '@/components/ui/Badge.vue'

const props = defineProps({ s: { type: Object, required: true } })
const statusQuery = useStatusQuery()

const downloads = computed(() => Object.entries(props.s.engineDownloads || {}))
const downloading = computed(() =>
  downloads.value.some(([, d]) => d && (d.stage === 'download' || d.stage === 'unpack')),
)
const configured = computed(() => props.s.wgConfigured || props.s.vlessConfigured)
const visible = computed(() => !props.s.enginesReady || !configured.value || downloading.value)
const enginesBusy = ref(false)
const certBusy = ref(false)
const proxyBusy = ref(false)

const progress = computed(() =>
  downloads.value
    .filter(([, d]) => d && d.stage !== 'done')
    .map(([name, d]) => ({
      name,
      percent: d.stage === 'download'
        ? Math.round((100 * (d.done || 0)) / Math.max(1, d.total || 1))
        : null,
      stage: d.stage,
      error: d.error,
    })),
)

async function downloadEngines() {
  enginesBusy.value = true
  try {
    await ensureEngines()
  } finally {
    enginesBusy.value = false
  }
}

async function trustCerts() {
  certBusy.value = true
  try {
    await installCerts()
  } finally {
    certBusy.value = false
  }
}

async function toggleProxy(enabled) {
  proxyBusy.value = true
  try {
    await post('/api/system-proxy', { enabled })
  } catch {
    /* тост уже показан */
  } finally {
    try {
      await statusQuery.refetch()
    } catch {
      /* следующий опрос обновит статус */
    }
    proxyBusy.value = false
  }
}
</script>

<template>
  <Card v-if="visible" title="Быстрый старт" description="Несколько шагов, чтобы всё заработало.">
    <template #icon>
      <Wifi class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="flex flex-wrap items-center gap-2 text-sm">
        <Badge :variant="s.enginesReady ? 'success' : 'secondary'">
          {{ s.enginesReady ? 'Движки готовы' : 'Движки не скачаны' }}
        </Badge>
        <Badge :variant="s.wgConfigured ? 'success' : 'secondary'">
          {{ s.wgConfigured ? 'WireGuard настроен' : 'WireGuard не настроен' }}
        </Badge>
        <Badge :variant="s.vlessConfigured ? 'success' : 'secondary'">
          {{ s.vlessConfigured ? 'VLESS настроен' : 'VLESS не настроен' }}
        </Badge>
        <Badge :variant="s.caTrusted ? 'success' : 'secondary'">
          {{ s.caTrusted ? 'CA доверенный' : 'CA не установлен' }}
        </Badge>
      </div>

      <p class="text-sm text-muted-foreground">
        <template v-if="!s.enginesReady">
          Скачайте движки wireproxy и xray — без них каналы не запустятся.
        </template>
        <template v-else-if="!configured">
          Откройте вкладку «Конфиги» и добавьте канал: импортируйте wg.conf в блоке
          WireGuard или вставьте vless://-ссылку в блоке VLESS.
        </template>
        <template v-else>
          Каналы настроены. Включите системный прокси, чтобы браузер пошёл через менеджер.
        </template>
      </p>

      <div v-for="item in progress" :key="item.name" class="space-y-1">
        <div class="flex justify-between text-xs text-muted-foreground">
          <span>{{ item.name }}</span>
          <span>{{ item.error || (item.percent != null ? item.percent + '%' : item.stage) }}</span>
        </div>
        <div class="h-1.5 overflow-hidden rounded-full bg-secondary">
          <div
            class="h-full rounded-full bg-primary transition-all"
            :style="{ width: (item.percent || 0) + '%' }"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <Button
          v-if="!s.enginesReady && !downloading"
          size="sm"
          :loading="enginesBusy"
          @click="downloadEngines"
        >
          <Download class="h-3.5 w-3.5" />
          Скачать движки
        </Button>
        <Button
          v-if="!s.caTrusted"
          size="sm"
          variant="outline"
          :loading="certBusy"
          @click="trustCerts"
        >
          <LockKeyhole class="h-3.5 w-3.5" />
          Установить сертификаты
        </Button>
        <label class="flex items-center gap-2 text-sm text-muted-foreground">
          <input
            type="checkbox"
            :checked="s.systemProxy?.enabled"
            :disabled="proxyBusy || (s.smartProxyReady === false && !s.systemProxy?.enabled)"
            class="h-4 w-4 rounded border-input"
            @change="toggleProxy($event.target.checked)"
          />
          Системный прокси
        </label>
      </div>
    </div>
  </Card>
</template>
