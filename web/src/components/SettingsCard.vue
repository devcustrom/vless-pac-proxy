<script setup>
import { computed, ref } from 'vue'
import { Blocks, Info, Power } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Switch from '@/components/ui/Switch.vue'
import PluginSetupDialog from '@/components/PluginSetupDialog.vue'

const props = defineProps({ s: { type: Object, required: true } })

const setupOpen = ref(false)
const localDomains = computed(() => !!props.s.plugins?.localDomains)
const telegram = computed(() => !!props.s.plugins?.telegram)
const zapret = computed(() => !!props.s.plugins?.zapret)
const zapretSupported = computed(() => props.s.zapret?.supported !== false)
const autostart = computed(() => !!props.s.autostart)
const autostartAction = useActionMutation(
  (enabled) => api('/api/autostart', { enabled }),
  [['status']],
)
const autostartLoading = computed(() => autostartAction.isLoading.value)
const disableAction = useActionMutation(
  () => api('/api/plugins', { localDomains: false }),
  [['status']],
)
const telegramAction = useActionMutation(
  (enabled) => api('/api/plugins', { telegram: enabled }),
  [['status']],
)
const zapretAction = useActionMutation(
  (enabled) => api('/api/plugins', { zapret: enabled }),
  [['status']],
)
const disableLoading = computed(() => disableAction.isLoading.value)
const telegramLoading = computed(() => telegramAction.isLoading.value)
const zapretLoading = computed(() => zapretAction.isLoading.value)

const core = computed(() => [
  {
    name: 'Умный прокси',
    description: 'Маршрутизация по доменам на 127.0.0.1:8081, списки и PAC.',
  },
  {
    name: 'Списки доменов',
    description: 'itdog + Re-filter RKN, обновление по расписанию.',
  },
  {
    name: 'Пул конфигов',
    description: 'Подписки, live-проверка и автопереключение Xray.',
  },
  {
    name: 'Каналы',
    description: 'WireGuard и Xray; включаются отдельно в карточке каналов.',
  },
])

async function toggleLocalDomains(value) {
  if (value) {
    setupOpen.value = true
    return
  }
  try {
    await disableAction.mutateAsync()
    notify('Плагин локальных доменов выключен')
  } catch {
    /* тост показан */
  }
}

async function toggleTelegram(value) {
  try {
    const result = await telegramAction.mutateAsync(value)
    if (value && result?.error) {
      notify(`Telegram включён, но не запустился: ${result.error}`, 'error')
    } else {
      notify(value ? 'Telegram-прокси включён' : 'Telegram-прокси выключен')
    }
  } catch {
    /* тост показан */
  }
}

async function toggleZapret(value) {
  try {
    await zapretAction.mutateAsync(value)
    notify(value ? 'Плагин Zapret включён' : 'Плагин Zapret выключен')
  } catch {
    /* тост показан */
  }
}

async function toggleAutostart(value) {
  try {
    await autostartAction.mutateAsync(value)
    notify(value ? 'Автозапуск включён' : 'Автозапуск выключен')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card
    title="Плагины"
    description="По умолчанию плагины выключены. Включайте только нужные."
  >
    <template #icon>
      <Blocks class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">Локальные домены</span>
            <Badge :variant="localDomains ? 'success' : 'secondary'">
              {{ localDomains ? 'включён' : 'выключен' }}
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            HTTP :{{ s.localProxy?.port }} / HTTPS :{{ s.localProxy?.httpsPort }}, свой CA,
            WebSocket. Слушатели работают только когда плагин включён.
          </p>
        </div>
        <Switch
          :model-value="localDomains"
          :disabled="disableLoading"
          @update:model-value="toggleLocalDomains"
        />
      </div>

      <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">Telegram-прокси</span>
            <Badge :variant="telegram ? 'success' : 'secondary'">
              {{ telegram ? 'включён' : 'выключен' }}
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            MTProto на 127.0.0.1:{{ s.telegram?.port || 1443 }}: WS-транспорт,
            WireGuard/VLESS и явный direct-режим. Настройки — во вкладке «Telegram».
          </p>
        </div>
        <Switch
          :model-value="telegram"
          :disabled="telegramLoading"
          @update:model-value="toggleTelegram"
        />
      </div>

      <div
        v-if="zapretSupported"
        class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3"
      >
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">Zapret</span>
            <Badge :variant="zapret ? 'success' : 'secondary'">
              {{ zapret ? 'включён' : 'выключен' }}
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            WinDivert и Windows-служба Zapret: general-пресеты, запуск и
            остановка из вкладки «Zapret».
          </p>
        </div>
        <Switch
          :model-value="zapret"
          :disabled="zapretLoading"
          @update:model-value="toggleZapret"
        />
      </div>

      <div class="space-y-2">
        <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <Info class="h-3.5 w-3.5" />
          Ядро (всегда включено)
        </div>
        <div
          v-for="item in core"
          :key="item.name"
          class="rounded-lg border px-3 py-2"
        >
          <div class="flex items-center justify-between gap-2">
            <span class="text-sm">{{ item.name }}</span>
            <Badge variant="outline">ядро</Badge>
          </div>
          <p class="mt-0.5 text-xs text-muted-foreground">{{ item.description }}</p>
        </div>
      </div>

      <p class="text-xs text-muted-foreground">
        Как добавить свой плагин или протокол — «Документация» → «Плагины и протоколы».
      </p>
    </div>
  </Card>

  <Card
    title="Приложение"
    description="Запуск менеджера при входе в систему — чтобы прокси и трей были доступны сразу."
  >
    <template #icon>
      <Power class="h-4 w-4 text-primary" />
    </template>
    <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
      <div class="min-w-0">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-sm font-medium">Запускать при входе</span>
          <Badge :variant="autostart ? 'success' : 'secondary'">
            {{ autostart ? 'включено' : 'выключено' }}
          </Badge>
        </div>
        <p class="mt-1 text-xs text-muted-foreground">
          Windows — HKCU Run, macOS — LaunchAgent. Панель и трей поднимутся
          автоматически после входа.
        </p>
      </div>
      <Switch
        :model-value="autostart"
        :disabled="autostartLoading"
        @update:model-value="toggleAutostart"
      />
    </div>
  </Card>

  <PluginSetupDialog
    :open="setupOpen"
    :s="s"
    @close="setupOpen = false"
    @enabled="setupOpen = false"
  />
</template>
