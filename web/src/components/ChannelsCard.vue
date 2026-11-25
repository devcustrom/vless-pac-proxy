<script setup>
import { computed, ref, watch } from 'vue'
import { Activity, Shield } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { notify, post } from '@/lib/store'
import { useStatusQuery } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })
const statusQuery = useStatusQuery()

const sysProxy = ref(!!props.s.systemProxy?.enabled)
const sysProxyBusy = ref(false)
const defaultBusy = ref('')
const channelBusy = ref('')
// Оптимистичное состояние: переключатель меняется сразу, а не после
// пятисекундного polling/refetch. При ошибке откатываемся.
const wgEnabled = ref(props.s.channels?.wireguard !== false)
const xrayEnabled = ref(props.s.channels?.xray !== false)
const vpnDefault = ref(props.s.vpnDefault || 'wireguard')
const smartReady = computed(() => props.s.smartProxyReady !== false)
watch(() => props.s.systemProxy?.enabled, (value) => {
  if (!sysProxyBusy.value) sysProxy.value = !!value
})
watch(() => props.s.channels?.wireguard, (value) => {
  if (!channelBusy.value) wgEnabled.value = value !== false
})
watch(() => props.s.channels?.xray, (value) => {
  if (!channelBusy.value) xrayEnabled.value = value !== false
})
watch(() => props.s.vpnDefault, (value) => {
  if (!defaultBusy.value) vpnDefault.value = value || 'wireguard'
})
const wgState = computed(() => {
  if (!props.s.wgConfigured) return { tone: 'secondary', text: 'не настроен' }
  return props.s.wgUp
    ? { tone: 'success', text: 'работает' }
    : { tone: 'destructive', text: props.s.proxyRunning ? 'канал не проходит трафик' : 'процесс отсутствует' }
})
const vlessState = computed(() => {
  if (!props.s.vlessConfigured) return { tone: 'secondary', text: 'не настроен' }
  return props.s.vlessUp
    ? { tone: 'success', text: 'работает' }
    : { tone: 'destructive', text: props.s.xrayRunning ? 'канал не проходит трафик' : 'процесс отсутствует' }
})

async function refreshStatus() {
  try {
    await statusQuery.refetch()
  } catch {
    /* статус обновится следующим опросом */
  }
}

async function setSystemProxy(value) {
  sysProxy.value = value
  sysProxyBusy.value = true
  try {
    await post('/api/system-proxy', { enabled: value })
  } catch {
    sysProxy.value = !value
  } finally {
    await refreshStatus()
    sysProxyBusy.value = false
  }
}

async function setDefault(value) {
  const previous = vpnDefault.value
  vpnDefault.value = value
  defaultBusy.value = value
  try {
    await post('/api/vless', { action: 'set-default', value })
  } catch {
    vpnDefault.value = previous
  } finally {
    await refreshStatus()
    vpnDefault.value = props.s.vpnDefault || previous
    defaultBusy.value = ''
  }
}

async function setChannel(channel, enabled) {
  const isWg = channel === 'wireguard'
  const previous = isWg ? wgEnabled.value : xrayEnabled.value
  if (isWg) wgEnabled.value = enabled
  else xrayEnabled.value = enabled
  channelBusy.value = channel
  try {
    await post('/api/channels', { channel, enabled })
  } catch {
    if (isWg) wgEnabled.value = previous
    else xrayEnabled.value = previous
  } finally {
    await refreshStatus()
    if (isWg) wgEnabled.value = props.s.channels?.wireguard !== false
    else xrayEnabled.value = props.s.channels?.xray !== false
    channelBusy.value = ''
  }
}

</script>

<template>
  <Card title="Подключение" description="Состояние каналов и системного прокси.">
    <div class="space-y-4">
      <div class="flex items-start gap-3">
        <div class="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-secondary">
          <Shield class="h-4 w-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">WireGuard</span>
            <Badge :variant="wgState.tone">{{ wgState.text }}</Badge>
            <span class="text-xs text-muted-foreground">
              IP: {{ s.exitIp || '—' }}
            </span>
          </div>
          <div class="mt-1 text-xs text-muted-foreground">
            {{ s.wgEndpoint || 'конфиг не задан' }}
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Switch
            data-testid="wireguard-channel"
            :model-value="wgEnabled"
            :disabled="!!channelBusy"
            @update:model-value="(value) => setChannel('wireguard', value)"
          />
        </div>
      </div>

      <div class="flex items-start gap-3">
        <div class="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-secondary">
          <Activity class="h-4 w-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm font-medium">VLESS</span>
            <Badge :variant="vlessState.tone">{{ vlessState.text }}</Badge>
            <span class="text-xs text-muted-foreground">
              IP: {{ s.exitIpVless || '—' }}
            </span>
          </div>
          <div class="mt-1 text-xs text-muted-foreground">
            {{ s.vlessEndpoint || 'конфиг не задан' }}
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Switch
            data-testid="xray-channel"
            :model-value="xrayEnabled"
            :disabled="!!channelBusy"
            @update:model-value="(value) => setChannel('xray', value)"
          />
        </div>
      </div>

      <details class="rounded-lg border bg-muted/30 px-3 py-2">
        <summary class="cursor-pointer text-sm font-medium">
          Основной канал для остальных доменов
        </summary>
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <span class="text-sm text-muted-foreground">
            Если авто-маршрут ещё не определён:
          </span>
          <div class="flex gap-1">
            <Button
              size="sm"
              :variant="vpnDefault === 'wireguard' ? 'default' : 'outline'"
              :loading="defaultBusy === 'wireguard'"
              :disabled="!!defaultBusy"
              @click="setDefault('wireguard')"
            >
              WireGuard
            </Button>
            <Button
              size="sm"
              :variant="vpnDefault === 'vless' ? 'default' : 'outline'"
              :loading="defaultBusy === 'vless'"
              :disabled="!!defaultBusy"
              @click="setDefault('vless')"
            >
              VLESS
            </Button>
          </div>
        </div>
      </details>

      <div class="flex items-center justify-between rounded-lg border bg-muted/30 px-3 py-2">
        <div>
          <div class="text-sm font-medium">Системный прокси</div>
          <div class="text-xs text-muted-foreground">
            {{ s.systemProxy?.enabled ? 'включён: ' + (s.systemProxy?.value || '') : 'выключен' }}
          </div>
          <div v-if="!smartReady" class="text-xs text-destructive">
            умный прокси не слушает 127.0.0.1:8081 — порт занят?
          </div>
        </div>
        <Switch
          :model-value="sysProxy"
          :disabled="sysProxyBusy || (!smartReady && !sysProxy)"
          @update:model-value="setSystemProxy"
        />
      </div>

      <RouterLink
        to="/channels"
        class="inline-flex text-xs text-primary hover:underline"
      >
        Настроить каналы и конфиги →
      </RouterLink>
    </div>
  </Card>
</template>
