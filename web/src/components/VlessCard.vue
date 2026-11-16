<script setup>
import { ref } from 'vue'
import { Activity, Plus, Save, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'

const props = defineProps({ s: { type: Object, required: true } })

const link = ref('')
const domain = ref('')
const busy = ref(false)

async function saveLink() {
  if (!link.value.trim()) return
  busy.value = true
  try {
    const result = await post('/api/vless', { action: 'set-link', link: link.value.trim() })
    link.value = ''
    const info = result.info || {}
    notify(`VLESS сохранён: ${info.name || info.address || 'ок'} (${info.security || '?'}/${info.network || '?'})`)
  } catch {
    /* тост уже показан */
  } finally {
    busy.value = false
  }
}

async function setDefault(value) {
  try {
    await post('/api/vless', { action: 'set-default', value })
    notify(`Основной канал: ${value === 'vless' ? 'VLESS' : 'WireGuard'}`)
  } catch {
    /* тост уже показан */
  }
}

async function addDomain() {
  const value = domain.value.trim()
  if (!value) return
  try {
    await post('/api/vless', { action: 'add-domain', domain: value })
    domain.value = ''
    notify('Домен добавлен в VLESS-список')
  } catch {
    /* тост уже показан */
  }
}

async function removeDomain(value) {
  try {
    await post('/api/vless', { action: 'remove-domain', domain: value })
    notify('Домен убран')
  } catch {
    /* тост уже показан */
  }
}
</script>

<template>
  <Card
    title="VLESS / SS — второй канал"
    description="Ссылка vless:// или ss:// (Reality, TLS, ws, grpc). Хранится в xray-config.json."
  >
    <template #icon>
      <Activity class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <Badge :variant="s.vlessConfigured ? 'success' : 'secondary'">
          {{ s.vlessConfigured ? 'настроен' : 'не настроен' }}
        </Badge>
        <span class="text-muted-foreground">IP: {{ s.exitIpVless || '—' }}</span>
      </div>

      <form class="flex flex-col gap-2 sm:flex-row" @submit.prevent="saveLink">
        <Input
          v-model="link"
          placeholder="vless://uuid@host:443?security=reality&sni=...&pbk=...#имя"
          autocomplete="off"
        />
        <Button type="submit" :disabled="busy || !link.trim()">
          <Save class="h-3.5 w-3.5" />
          Сохранить
        </Button>
      </form>

      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted-foreground">VPN-домены идут через:</span>
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

      <div class="space-y-2">
        <Label>Домены только через VLESS</Label>
        <form class="flex gap-2" @submit.prevent="addDomain">
          <Input v-model="domain" placeholder="example.com" autocomplete="off" />
          <Button type="submit" variant="outline" size="sm">
            <Plus class="h-3.5 w-3.5" />
            Добавить
          </Button>
        </form>
        <ul v-if="s.vlessDomains?.length" class="divide-y rounded-lg border">
          <li
            v-for="item in s.vlessDomains"
            :key="item"
            class="flex items-center justify-between px-3 py-2 text-sm"
          >
            <span class="font-mono text-xs">{{ item }}</span>
            <Button size="sm" variant="ghost" @click="removeDomain(item)">
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </li>
        </ul>
        <p v-else class="text-xs text-muted-foreground">Список пуст.</p>
      </div>
    </div>
  </Card>
</template>
