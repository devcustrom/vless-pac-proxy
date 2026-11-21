<script setup>
import { computed, ref } from 'vue'
import { Blocks, Copy, Power, RefreshCw } from 'lucide-vue-next'
import { api, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'
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
  <Card title="Плагины" description="Включайте только нужные функции. Остальные не занимают вкладки.">
    <template #icon>
      <Blocks class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
        <div class="min-w-0">
          <span class="text-sm font-medium">Локальные домены</span>
          <p class="mt-1 text-xs text-muted-foreground">
            HTTPS-домены, локальный CA и hosts. Настройки — во вкладке «Локальные домены».
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
          <span class="text-sm font-medium">Telegram-прокси</span>
          <p class="mt-1 text-xs text-muted-foreground">
            Локальный MTProto-прокси с WS/VPN/direct. Настройки — во вкладке «Telegram».
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
          <span class="text-sm font-medium">Обход DPI (Zapret)</span>
          <p class="mt-1 text-xs text-muted-foreground">
            Zapret 2, каталог стратегий и WinDivert. Настройки — во вкладке «Обход DPI».
          </p>
        </div>
        <Switch
          :model-value="zapret"
          :disabled="zapretLoading"
          @update:model-value="toggleZapret"
        />
      </div>

      <details class="rounded-lg border px-3 py-2">
        <summary class="cursor-pointer text-xs font-medium text-muted-foreground">
          Что входит в ядро
        </summary>
        <ul class="mt-2 space-y-1 text-xs text-muted-foreground">
          <li v-for="item in core" :key="item.name">
            <b class="text-foreground">{{ item.name }}</b> — {{ item.description }}
          </li>
        </ul>
      </details>

      <p class="text-xs text-muted-foreground">
        Свой плагин или протокол — «Документация» → «Плагины и протоколы».
      </p>
    </div>
  </Card>

  <Card
    title="Основное"
    description="Автозапуск, идентификатор устройства и проверка сайтов."
  >
    <template #icon>
      <Power class="h-4 w-4 text-primary" />
    </template>

    <div class="flex items-center justify-between gap-3 rounded-lg border bg-muted/30 px-3 py-3">
      <div class="min-w-0">
        <span class="text-sm font-medium">Запускать при входе</span>
        <p class="mt-1 text-xs text-muted-foreground">
          Панель и трей поднимутся автоматически после входа в систему.
        </p>
      </div>
      <Switch
        :model-value="autostart"
        :disabled="autostartLoading"
        @update:model-value="toggleAutostart"
      />
    </div>

    <details class="mt-3 rounded-lg border bg-muted/30 px-3 py-2">
      <summary class="cursor-pointer text-sm font-medium">HWID устройства</summary>
      <div class="mt-2 space-y-2">
        <code class="block break-all rounded bg-background/60 px-2 py-1 text-xs">
          {{ s.hwid || 'генерируется…' }}
        </code>
        <p class="text-xs text-muted-foreground">
          Передаётся в заголовке x-hwid при загрузке подписок и сохраняется между обновлениями.
        </p>
        <div class="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" @click="copyHwid">
            <Copy class="h-3.5 w-3.5" />
            Копировать
          </Button>
          <Button size="sm" variant="ghost" :loading="hwidLoading" @click="regenerateHwid">
            <RefreshCw class="h-3.5 w-3.5" />
            Сменить HWID
          </Button>
        </div>
      </div>
    </details>

    <details class="mt-3 rounded-lg border bg-muted/30 px-3 py-2">
      <summary class="cursor-pointer text-sm font-medium">Проверка сайтов</summary>
      <div class="mt-3 space-y-3">
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <span class="text-sm">Автопроверка новых сайтов</span>
            <p class="mt-1 text-xs text-muted-foreground">
              Раз в 90 секунд проверять новые домены из «Недавних сайтов».
            </p>
          </div>
          <Switch
            :model-value="autoCheck"
            :disabled="autoCheckLoading"
            @update:model-value="toggleAutoCheckSites"
          />
        </div>
        <div class="flex items-center justify-between gap-3">
          <div class="min-w-0">
            <span class="text-sm">Добавлять недоступные в VPN</span>
            <p class="mt-1 text-xs text-muted-foreground">
              Если сайт открывается только через VPN — добавить домен в VPN-список.
            </p>
          </div>
          <Switch
            :model-value="autoAdd"
            :disabled="autoCheckLoading || !autoCheck"
            @update:model-value="toggleAutoAdd"
          />
        </div>
      </div>
    </details>
  </Card>

  <PluginSetupDialog
    :open="setupOpen"
    :s="s"
    @close="setupOpen = false"
    @enabled="setupOpen = false"
  />
</template>
