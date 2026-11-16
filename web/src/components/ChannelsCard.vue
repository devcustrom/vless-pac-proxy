<script setup>
import { computed, ref, watch } from 'vue'
import { Activity, RotateCcw, Shield } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const sysProxy = ref(!!props.s.systemProxy?.enabled)
watch(() => props.s.systemProxy?.enabled, (value) => {
  sysProxy.value = !!value
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

async function setSystemProxy(value) {
  sysProxy.value = value
  try {
    await post('/api/system-proxy', { enabled: value })
  } catch {
    sysProxy.value = !value
  }
}

async function setDefault(value) {
  try {
    await post('/api/vless', { action: 'set-default', value })
  } catch {
    /* тост показан */
  }
}

async function setChannel(channel, enabled) {
  try {
    await post('/api/channels', { channel, enabled })
  } catch {
    /* тост показан */
  }
}

async function restartChannel(channel) {
  try {
    await post('/api/channels', { channel, restart: true })
    notify(channel === 'wireguard' ? 'WireGuard перезапускается' : 'Xray перезапускается')
  } catch {
    /* тост показан */
  }
}
</script>

<template>
  <Card>
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
          <Button
            size="icon"
            variant="ghost"
            :disabled="s.channels?.wireguard === false"
            title="Перезапустить WireGuard"
            @click="restartChannel('wireguard')"
          >
            <RotateCcw class="h-4 w-4" />
          </Button>
          <Switch
            :model-value="s.channels?.wireguard !== false"
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
            основной канал: {{ s.vpnDefault === 'vless' ? 'VLESS' : 'WireGuard' }}
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Button
            size="icon"
            variant="ghost"
            :disabled="s.channels?.xray === false"
            title="Перезапустить Xray"
            @click="restartChannel('xray')"
          >
            <RotateCcw class="h-4 w-4" />
          </Button>
          <Switch
            :model-value="s.channels?.xray !== false"
            @update:model-value="(value) => setChannel('xray', value)"
          />
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2">
        <span class="text-sm text-muted-foreground">VPN-домены идут через:</span>
        <div class="flex gap-1">
          <Button
            size="sm"
            :variant="s.vpnDefault === 'wireguard' ? 'default' : 'outline'"
            @click="setDefault('wireguard')"
          >
            WireGuard
          </Button>
          <Button
            size="sm"
            :variant="s.vpnDefault === 'vless' ? 'default' : 'outline'"
            @click="setDefault('vless')"
          >
            VLESS
          </Button>
        </div>
      </div>

      <div class="flex items-center justify-between rounded-lg border bg-muted/30 px-3 py-2">
        <div>
          <div class="text-sm font-medium">Системный прокси</div>
          <div class="text-xs text-muted-foreground">
            {{ s.systemProxy?.enabled ? 'включён: ' + (s.systemProxy?.value || '') : 'выключен' }}
          </div>
        </div>
        <Switch :model-value="sysProxy" @update:model-value="setSystemProxy" />
      </div>
    </div>
  </Card>
</template>
