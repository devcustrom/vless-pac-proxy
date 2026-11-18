<script setup>
import { ref } from 'vue'
import { Activity, Save, Trash2 } from 'lucide-vue-next'
import { notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'

const props = defineProps({ s: { type: Object, required: true } })

const link = ref('')
const busy = ref(false)
const deleting = ref(false)

async function saveLink() {
  if (!link.value.trim()) return
  busy.value = true
  try {
    const result = await post('/api/vless', { action: 'set-link', link: link.value.trim() })
    link.value = ''
    const info = result.info || {}
    notify(`Канал сохранён: ${info.name || info.address || 'ок'} (${info.protocol || ''} ${info.security || '?'}/${info.network || '?'})`)
  } catch {
    /* тост уже показан */
  } finally {
    busy.value = false
  }
}

async function removeConfig() {
  if (!confirm('Удалить активный VLESS/VMess/Trojan/SS-конфиг? Подписки и пул останутся.')) return
  deleting.value = true
  try {
    await post('/api/vless', { action: 'remove-link' })
    notify('VLESS-конфиг удалён')
  } catch {
    /* тост уже показан */
  } finally {
    deleting.value = false
  }
}
</script>

<template>
  <Card
    title="VLESS / VMess / Trojan / SS"
    description="Ссылка vless://, vmess://, trojan:// или ss://. Конфиг сохраняется в xray-config.json, движок перезапускается."
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
        <Button
          v-if="s.vlessConfigured"
          size="sm"
          variant="ghost"
          :loading="deleting"
          :disabled="busy || deleting"
          @click="removeConfig"
        >
          <Trash2 class="h-3.5 w-3.5" />
          Удалить конфиг
        </Button>
      </div>

      <form class="flex flex-col gap-2 sm:flex-row" @submit.prevent="saveLink">
        <Input
          v-model="link"
          placeholder="vless://uuid@host:443?... или vmess://... / trojan://... / ss://..."
          :disabled="deleting"
          autocomplete="off"
        />
        <Button type="submit" :loading="busy" :disabled="!link.trim() || deleting">
          <Save class="h-3.5 w-3.5" />
          Сохранить
        </Button>
      </form>

      <p class="text-xs text-muted-foreground">
        Домены «только VLESS» и «только WireGuard» — кнопка «Маршрутизация
        доменов» на вкладке «Обзор». Подписки и пул конфигов — в карточке ниже.
      </p>
    </div>
  </Card>
</template>
