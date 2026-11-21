<script setup>
import { computed, ref } from 'vue'
import { Blocks, Copy, Info, Power, RefreshCw } from 'lucide-vue-next'
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
const hwidAction = useActionMutation(
  () => api('/api/hwid', { regenerate: true }),
  [['status']],
)
const hwidLoading = computed(() => hwidAction.isLoading.value)
const autoCheck = computed(() => props.s.autoCheckSites !== false)
const autoAdd = computed(() => props.s.autoAddUnavailable !== false)
const autoCheckAction = useActionMutation(
  (payload) => api('/api/auto', payload),
  [['status']],
)
const autoCheckLoading = computed(() => autoCheckAction.isLoading.value)
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
    notify(value ? 'Обход DPI включён' : 'Обход DPI выключен')
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

async function copyHwid() {
  try {
    await navigator.clipboard.writeText(props.s.hwid || '')
    notify('HWID скопирован')
  } catch (error) {
    notify(error.message || 'Не удалось скопировать HWID', 'error')
  }
}

async function regenerateHwid() {
  if (
    !confirm(
      'Сменить HWID? Подписки, привязанные к текущему устройству, могут потребовать повторной авторизации.',
    )
  ) {
    return
  }
  try {
    await hwidAction.mutateAsync()
    notify('HWID изменён')
  } catch {
    /* тост показан */
  }
}

async function toggleAutoCheckSites(value) {
  try {
    await autoCheckAction.mutateAsync({ autoCheckSites: value })
    notify(value ? 'Автопроверка новых сайтов включена' : 'Автопроверка выключена')
  } catch {
    /* тост показан */
  }
}

async function toggleAutoAdd(value) {
  try {
    await autoCheckAction.mutateAsync({ autoAddUnavailable: value })
    notify(value ? 'Недоступные сайты будут добавляться в VPN' : 'Автодобавление выключено')
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
            <span class="text-sm font-medium">Обход DPI (Zapret)</span>
            <Badge :variant="zapret ? 'success' : 'secondary'">
              {{ zapret ? 'включён' : 'выключен' }}
            </Badge>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            WinDivert и Windows-служба Zapret: zapret2, каталог стратегий Zaprett,
            автоподбор и запуск из вкладки «Обход DPI».
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

    <div class="mt-3 rounded-lg border bg-muted/30 px-3 py-3">
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm font-medium">HWID устройства</span>
        <Badge variant="outline">x-hwid</Badge>
      </div>
      <code class="mt-1 block break-all rounded bg-background/60 px-2 py-1 text-xs">
        {{ s.hwid || 'генерируется…' }}
      </code>
      <p class="mt-1 text-xs text-muted-foreground">
        Передаётся в заголовках при загрузке подписок. Один и тот же ID
        сохраняется между обновлениями и перезапусками.
      </p>
      <div class="mt-2 flex flex-wrap gap-2">
        <Button size="sm" variant="outline" @click="copyHwid">
          <Copy class="h-3.5 w-3.5" />
          Копировать
        </Button>
        <Button size="sm" variant="ghost" :loading="hwidLoading" @click="regenerateHwid">
          <RefreshCw class="h-3.5 w-3.5" />
          Сменить HWID
        </Button>
      </div>
      <div class="mt-3 flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <div class="text-sm font-medium">Автопроверка новых сайтов</div>
          <p class="mt-1 text-xs text-muted-foreground">
            Фоновая задача раз в 90 секунд проверяет новые домены из «Недавних
            сайтов» напрямую, через VPN и Zapret.
          </p>
        </div>
        <Switch
          :model-value="autoCheck"
          :disabled="autoCheckLoading"
          @update:model-value="toggleAutoCheckSites"
        />
      </div>

      <div class="mt-3 flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <div class="text-sm font-medium">Добавлять недоступные в VPN</div>
          <p class="mt-1 text-xs text-muted-foreground">
            Если сайт не открывается напрямую, но открывается через VPN, домен
            автоматически попадёт в VPN-список.
          </p>
        </div>
        <Switch
          :model-value="autoAdd"
          :disabled="autoCheckLoading || !autoCheck"
          @update:model-value="toggleAutoAdd"
        />
      </div>
    </div>
  </Card>

  <PluginSetupDialog
    :open="setupOpen"
    :s="s"
    @close="setupOpen = false"
    @enabled="setupOpen = false"
  />
</template>
