<script setup>
import { ref } from 'vue'
import { Plus, Search } from 'lucide-vue-next'
import { api, notify, post, refresh } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'

const props = defineProps({ s: { type: Object, required: true } })

const domain = ref('')
const result = ref(null)
const busy = ref(false)
const adding = ref('')

const VERDICTS = {
  'vpn-helps': ['success', 'через VPN открывается — добавляйте'],
  'direct-only': ['destructive', 'через VPN хуже — не добавляйте'],
  'both-ok': ['outline', 'работает и напрямую — VPN не нужен'],
  'zapret-helps': ['success', 'через Zapret открывается — VPN не нужен'],
  challenge: ['warning', '403 на обоих путях — анти-бот; в браузере через VPN часто работает'],
  'blocked-everywhere': ['destructive', 'не отвечает даже через VPN'],
}

function verdict(code) {
  return VERDICTS[code] || ['outline', code]
}

function codeVariant(code) {
  return code && code !== '000' && code !== '403' && code !== '451' ? 'success' : 'destructive'
}

function codeText(code) {
  return code === '000' ? 'нет ответа' : code
}

function zapretText(item) {
  if (!item?.zapretSupported) return 'нет'
  if (!item?.zapretRunning) return 'выключен'
  return codeText(item.zapret)
}

function zapretVariant(item) {
  if (!item?.zapretSupported) return 'outline'
  if (!item?.zapretRunning) return 'secondary'
  return codeVariant(item.zapret)
}

async function check() {
  const value = domain.value.trim()
  if (!value) return
  busy.value = true
  try {
    result.value = await api('/api/check', { domain: value })
    await refresh()
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    busy.value = false
  }
}

async function addToVpn(value) {
  adding.value = value
  try {
    await post('/api/extra', { action: 'add', domain: value })
    notify(value + ' добавлен в VPN-список')
  } catch {
    /* тост уже показан */
  } finally {
    adding.value = ''
  }
}
</script>

<template>
  <Card title="Проверка сайта" description="Сравним напрямую, через VPN и Zapret, добавим в список одной кнопкой.">
    <template #icon>
      <Search class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <form class="flex gap-2" @submit.prevent="check">
        <Input v-model="domain" placeholder="example.com" autocomplete="off" />
        <Button type="submit" :loading="busy">
          <Search class="h-3.5 w-3.5" />
          Проверить
        </Button>
      </form>

      <div v-if="result" class="space-y-2 rounded-lg border bg-muted/30 px-3 py-2">
        <div class="flex flex-wrap items-center gap-2 text-sm">
          <span>Напрямую:</span>
          <Badge :variant="codeVariant(result.direct)">{{ codeText(result.direct) }}</Badge>
          <span>Через VPN:</span>
          <Badge :variant="codeVariant(result.vpn)">{{ codeText(result.vpn) }}</Badge>
          <span>Через Zapret:</span>
          <Badge :variant="zapretVariant(result)">{{ zapretText(result) }}</Badge>
        </div>
        <p v-if="result.zapretRunning" class="text-xs text-muted-foreground">
          Zapret работает прозрачно: прямой запрос уже проходит через него.
        </p>
        <div class="flex flex-wrap items-center gap-2">
          <Badge :variant="verdict(result.verdict)[0]">{{ verdict(result.verdict)[1] }}</Badge>
          <Button
            v-if="['vpn-helps', 'challenge'].includes(result.verdict) && !(s.extra || []).includes(result.domain)"
            size="sm"
            :loading="adding === result.domain"
            :disabled="!!adding"
            @click="addToVpn(result.domain)"
          >
            <Plus class="h-3.5 w-3.5" />
            Добавить в VPN-список
          </Button>
        </div>
      </div>

      <div v-if="(s.checks || []).length" class="space-y-1">
        <div class="text-xs font-medium text-muted-foreground">Последние проверки</div>
        <ul class="max-h-48 divide-y overflow-auto rounded-lg border text-sm scrollbar-thin">
          <li
            v-for="item in (s.checks || []).filter((c) => c.verdict !== 'both-ok').slice(0, 10)"
            :key="item.at + item.domain"
            class="flex items-center gap-2 px-3 py-2"
          >
            <span class="min-w-0 flex-1 truncate">{{ item.domain }}</span>
            <Badge :variant="codeVariant(item.direct)">{{ codeText(item.direct) }}</Badge>
            <Badge :variant="codeVariant(item.vpn)">{{ codeText(item.vpn) }}</Badge>
            <Badge :variant="zapretVariant(item)">{{ zapretText(item) }}</Badge>
            <Button
              v-if="['vpn-helps', 'challenge'].includes(item.verdict) && !(s.extra || []).includes(item.domain)"
              size="sm"
              variant="outline"
              :loading="adding === item.domain"
              :disabled="!!adding"
              @click="addToVpn(item.domain)"
            >
              добавить
            </Button>
          </li>
        </ul>
      </div>
    </div>
  </Card>
</template>
