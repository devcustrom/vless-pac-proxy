<script setup>
import { computed } from 'vue'
import {
  AlertTriangle,
  ArrowRight,
  Loader2,
  Power,
  ShieldCheck,
  ShieldOff,
} from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import SimpleTelegramCard from '@/components/SimpleTelegramCard.vue'
import UpdateCard from '@/components/UpdateCard.vue'

const props = defineProps({ s: { type: Object, required: true } })
const emit = defineEmits(['advanced'])

const simple = computed(() => props.s.simple || {})
const busy = computed(() => !!simple.value.busy)
const poolTesting = computed(() => !!props.s.poolTesting)
const autoConfig = computed(() => !!props.s.autoConfigRunning)
const working = computed(() => !!props.s.wgUp || !!props.s.vlessUp)
const enginesRunning = computed(
  () => !!props.s.proxyRunning || !!props.s.xrayRunning,
)
const vpnOn = computed(() => !!props.s.systemProxy?.enabled && working.value)
const connecting = computed(
  () => !!props.s.systemProxy?.enabled && !working.value && enginesRunning.value,
)
const enabledConfigs = computed(() => {
  let count = 0
  if (props.s.wgConfigured) count += 1
  if (props.s.vlessConfigured) count += 1
  return count
})
const providers = computed(() => {
  const items = []
  if (props.s.wgUp) items.push('WireGuard')
  if (props.s.vlessUp) items.push('VLESS')
  return items
})
const exitIp = computed(() =>
  props.s.vlessUp ? props.s.exitIpVless : props.s.wgUp ? props.s.exitIp : '',
)
const progressText = computed(() => {
  if (simple.value.message) return simple.value.message
  if (poolTesting.value) return 'Проверяю VPN-конфиги из подписок…'
  if (autoConfig.value) return 'Выбираю самый быстрый конфиг…'
  return ''
})
const error = computed(() => simple.value.error || '')
const statusTitle = computed(() => {
  if (busy.value) return 'Включаю VPN'
  if (vpnOn.value) return 'VPN включён'
  if (connecting.value) return 'Подключаюсь…'
  return 'VPN выключен'
})
const statusText = computed(() => {
  if (busy.value) return 'Это может занять несколько минут: ищу рабочий конфиг и проверяю трафик.'
  if (vpnOn.value) return 'Весь трафик идёт через защищённый канал.'
  if (connecting.value) return 'Канал поднят, проверяю доступ в интернет.'
  if (enabledConfigs.value === 0) {
    return 'Конфигов пока нет — при включении автоматически загружу их из подписок.'
  }
  return 'Нажмите «Включить VPN» — приложение выберет лучший конфиг само.'
})

async function enable() {
  try {
    await post('/api/simple/enable', {})
    notify('Включаю VPN…')
  } catch {
    /* тост уже показан */
  }
}

async function disable() {
  try {
    await post('/api/simple/disable', {})
    notify('VPN выключен')
  } catch {
    /* тост уже показан */
  }
}

function toggle() {
  if (vpnOn.value) disable()
  else enable()
}
</script>

<template>
  <div class="mx-auto flex min-h-[68vh] max-w-xl flex-col justify-center gap-4 py-4">
    <div class="w-full rounded-3xl border bg-background p-7 shadow-sm sm:p-10">
      <div class="flex flex-col items-center text-center">
        <div
          class="flex h-20 w-20 items-center justify-center rounded-full transition-colors"
          :class="
            vpnOn
              ? 'bg-success/15 text-success'
              : busy
                ? 'bg-primary/10 text-primary'
                : 'bg-secondary text-muted-foreground'
          "
        >
          <Loader2 v-if="busy" class="h-9 w-9 animate-spin" />
          <ShieldCheck v-else-if="vpnOn" class="h-10 w-10" />
          <ShieldOff v-else class="h-10 w-10" />
        </div>

        <h2 class="mt-5 text-2xl font-semibold">{{ statusTitle }}</h2>
        <p class="mt-2 max-w-md text-sm text-muted-foreground">{{ statusText }}</p>

        <div v-if="vpnOn" class="mt-4 flex flex-wrap items-center justify-center gap-2">
          <Badge variant="success">защищено</Badge>
          <Badge v-for="item in providers" :key="item" variant="outline">{{ item }}</Badge>
          <span v-if="exitIp" class="text-xs text-muted-foreground">IP {{ exitIp }}</span>
        </div>

        <Button
          data-testid="simple-enable"
          size="lg"
          class="mt-7 h-14 w-full text-base"
          :variant="vpnOn ? 'outline' : 'default'"
          :loading="busy"
          :disabled="busy"
          @click="toggle"
        >
          <Power v-if="!busy" class="h-5 w-5" />
          {{ vpnOn ? 'Выключить VPN' : 'Включить VPN' }}
        </Button>

        <div v-if="progressText" class="mt-5 w-full rounded-xl border bg-muted/30 px-4 py-3">
          <div class="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 class="h-4 w-4 animate-spin" />
            {{ progressText }}
          </div>
          <div class="mt-3 h-1.5 overflow-hidden rounded-full bg-muted">
            <div class="h-full w-1/3 animate-pulse rounded-full bg-primary" />
          </div>
        </div>

        <div
          v-if="error"
          class="mt-5 flex w-full items-start gap-2 rounded-xl border border-destructive/40 bg-destructive/10 px-4 py-3 text-left text-xs text-destructive"
        >
          <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
          <div class="min-w-0">
            <p>{{ error }}</p>
            <button class="mt-1 font-medium underline" type="button" @click="enable">
              Повторить
            </button>
          </div>
        </div>

        <button
          data-testid="simple-advanced"
          type="button"
          class="mt-7 inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
          @click="emit('advanced')"
        >
          Расширенный режим
          <ArrowRight class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <SimpleTelegramCard :s="s" @advanced="emit('advanced')" />

    <div data-testid="simple-update">
      <UpdateCard />
    </div>
  </div>
</template>
