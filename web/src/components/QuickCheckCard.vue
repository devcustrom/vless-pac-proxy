<script setup>
import { computed, ref } from 'vue'
import { Search, ShieldPlus, Wand2 } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { api, notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import StrategyDialog from '@/components/StrategyDialog.vue'
import { isValidHostname, normalizeDomainInput } from '@/lib/validators'

const props = defineProps({ s: { type: Object, required: true } })

const domain = ref('')
const result = ref(null)
const busy = ref(false)
const adding = ref(false)
const strategyFor = ref('')
const blocked = (code) => !code || ['000', '403', '451'].includes(code)
const directBlocked = computed(() => !!result.value && blocked(result.value.direct))
const vpnHelps = computed(() => !!result.value && !blocked(result.value.vpn))
const inAuto = computed(() => {
  if (!result.value) return false
  const domain = result.value.domain.toLowerCase()
  return (props.s.extra || []).some((item) => item.toLowerCase() === domain)
})
const canAddAuto = computed(
  () => directBlocked.value && vpnHelps.value && !inAuto.value,
)
const canGenerate = computed(
  () =>
    !!props.s.plugins?.zapret &&
    !!props.s.zapret?.supported &&
    !!props.s.zapret?.bundleInstalled &&
    !!props.s.zapret?.blockcheck,
)

function codeVariant(code) {
  return code && code !== '000' && code !== '403' && code !== '451' ? 'success' : 'destructive'
}

function codeText(code) {
  return code === '000' ? 'нет' : code
}

function onPaste(event) {
  const text = event.clipboardData?.getData('text') || ''
  const cleaned = normalizeDomainInput(text)
  if (!cleaned) return
  event.preventDefault()
  domain.value = cleaned
}

async function addToAuto() {
  if (!result.value) return
  adding.value = true
  try {
    await post('/api/extra', { action: 'add', domain: result.value.domain })
    notify(`Домен ${result.value.domain} добавлен в авто-список`)
  } catch {
    /* тост показан */
  } finally {
    adding.value = false
  }
}

function vpnBadges(item) {
  if (!item) return []
  const parts = []
  if (item.wireguard) parts.push({ label: 'WG', code: item.wireguard.code })
  if (item.vless) parts.push({ label: 'VLESS', code: item.vless.code })
  if (!parts.length) parts.push({ label: 'VPN', code: item.vpn })
  return parts
}

function bestRouteLabel(item) {
  if (!item?.bestRoute) return ''
  const name =
    item.bestRoute === 'wireguard'
      ? 'WireGuard'
      : item.bestRoute === 'vless'
        ? 'VLESS'
        : 'напрямую'
  return item.bestLatencyMs ? `${name} · ${item.bestLatencyMs} мс` : name
}

async function check() {
  const value = normalizeDomainInput(domain.value)
  domain.value = value
  if (!value) return
  if (!isValidHostname(value)) {
    notify('Не удалось выделить корректный домен из ссылки', 'error')
    return
  }
  busy.value = true
  try {
    result.value = await api('/api/check', { domain: value })
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Card title="Проверить домен" description="Быстрая проверка напрямую, через WireGuard/VLESS и Zapret.">
    <template #icon>
      <Search class="h-4 w-4 text-primary" />
    </template>
    <form class="flex gap-2" @submit.prevent="check">
      <Input
        v-model="domain"
        placeholder="example.com или ссылка из браузера"
        autocomplete="off"
        @paste="onPaste"
        @blur="domain = normalizeDomainInput(domain)"
      />
      <Button type="submit" :loading="busy">
        <Search class="h-3.5 w-3.5" />
        Проверить
      </Button>
    </form>
    <div v-if="result" class="mt-3 space-y-2 rounded-lg border bg-muted/30 px-3 py-2 text-sm">
      <div class="flex flex-wrap items-center gap-1.5">
        <Badge :variant="codeVariant(result.direct)">напрямую {{ codeText(result.direct) }}</Badge>
        <Badge
          v-for="part in vpnBadges(result)"
          :key="part.label"
          :variant="codeVariant(part.code)"
        >
          {{ part.label }} {{ codeText(part.code) }}
        </Badge>
        <Badge :variant="result.zapretRunning ? codeVariant(result.zapret) : 'secondary'">
          Zapret {{ result.zapretRunning ? codeText(result.zapret) : 'выкл' }}
        </Badge>
        <Badge v-if="result.bestRoute" variant="secondary">
          лучший: {{ bestRouteLabel(result) }}
        </Badge>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <RouterLink
          :to="{ path: '/sites', query: { q: result.domain } }"
          class="text-xs text-primary hover:underline"
        >
          Открыть в истории сайтов
        </RouterLink>
        <Button
          v-if="canAddAuto"
          size="sm"
          variant="outline"
          :loading="adding"
          :disabled="adding"
          @click="addToAuto"
        >
          <ShieldPlus class="h-3.5 w-3.5" />
          Добавить в авто
        </Button>
        <Badge v-else-if="directBlocked && inAuto" variant="secondary">уже в авто</Badge>
        <Button
          v-if="canGenerate && directBlocked"
          size="sm"
          variant="outline"
          @click="strategyFor = result.domain"
        >
          <Wand2 class="h-3.5 w-3.5" />
          Подобрать стратегии Zapret
        </Button>
      </div>
    </div>

    <StrategyDialog
      :open="!!strategyFor"
      :domain="strategyFor"
      :s="s"
      @close="strategyFor = ''"
    />
  </Card>
</template>
