<script setup>
import { reactive, ref } from 'vue'
import { ExternalLink, Globe, LockKeyhole, Pencil, Plus, ShieldCheck, Trash2 } from 'lucide-vue-next'
import { api, installCerts, notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'

const props = defineProps({ s: { type: Object, required: true } })

const domain = ref('')
const port = ref('')
const probes = reactive({})

async function prepareHosts() {
  try {
    const result = await api('/api/local-domains/hosts', {})
    notify('Скрипт hosts: ' + result.script)
  } catch (error) {
    notify(error.message, 'error')
  }
}

async function editHosts() {
  try {
    await api('/api/local-domains/hosts/edit', {})
    notify('Открыт редактор hosts (подтвердите UAC/пароль)')
  } catch (error) {
    notify(error.message, 'error')
  }
}

const lp = () => props.s.localProxy || {}
const httpPort = () => (lp().port === 80 ? '' : `:${lp().port}`)
const httpsPort = () => (lp().httpsPort === 443 ? '' : `:${lp().httpsPort}`)

async function addDomain() {
  const name = domain.value.trim()
  const target = Number(port.value.trim())
  if (!name || !target) {
    notify('Укажите домен и порт', 'error')
    return
  }
  try {
    await post('/api/local-domains', { action: 'add', domain: name, port: target })
    domain.value = ''
    port.value = ''
    notify(`Домен добавлен: http://${name}`)
  } catch {
    /* тост уже показан */
  }
}

async function removeDomain(name) {
  try {
    await post('/api/local-domains', { action: 'remove', domain: name })
    delete probes[name]
    notify('Домен убран')
  } catch {
    /* тост уже показан */
  }
}

async function probe(name) {
  try {
    const result = await api('/api/local-domains/probe', { domain: name })
    probes[name] = result
  } catch (error) {
    probes[name] = { status: 0, error: error.message }
  }
}
</script>

<template>
  <Card
    title="Локальные домены"
    description="Домен → локальный порт, HTTP/HTTPS через свой CA. WebSocket поддерживается."
  >
    <template #icon>
      <Globe class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <div
        v-if="!s.plugins?.localDomains"
        class="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400"
      >
        Плагин выключен: слушатели остановлены. Включите его во вкладке
        «Настройки» → «Плагины».
      </div>

      <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        <Badge variant="outline">HTTP :{{ lp().port ?? '—' }}</Badge>
        <Badge variant="outline">HTTPS :{{ lp().httpsPort ?? '—' }}</Badge>
        <Badge :variant="s.caTrusted ? 'success' : 'secondary'">
          {{ s.caTrusted ? 'CA доверенный' : 'CA не установлен' }}
        </Badge>
        <Button v-if="!s.caTrusted" size="sm" variant="outline" @click="installCerts">
          <LockKeyhole class="h-3.5 w-3.5" />
          Установить сертификаты
        </Button>
      </div>

      <div v-if="lp().error || lp().httpsError" class="space-y-1 text-xs text-destructive">
        <div v-if="lp().error">http: {{ lp().error }}</div>
        <div v-if="lp().httpsError">https: {{ lp().httpsError }}</div>
      </div>

      <ul v-if="lp().domains?.length" class="divide-y rounded-lg border">
        <li v-for="item in lp().domains" :key="item.domain" class="px-3 py-2">
          <div class="flex flex-wrap items-center gap-2">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2 text-sm">
                <a
                  :href="`http://${item.domain}${httpPort()}`"
                  target="_blank"
                  class="inline-flex items-center gap-1 font-medium hover:underline"
                >
                  {{ item.domain }}
                  <ExternalLink class="h-3 w-3" />
                </a>
                <span class="text-xs text-muted-foreground">→ 127.0.0.1:{{ item.port }}</span>
              </div>
              <div v-if="probes[item.domain]" class="text-xs">
                <Badge :variant="probes[item.domain].status ? 'success' : 'destructive'">
                  {{ probes[item.domain].status || 'нет ответа' }}
                  <template v-if="probes[item.domain].ms != null">· {{ probes[item.domain].ms }} мс</template>
                </Badge>
              </div>
            </div>
            <Button size="sm" variant="outline" @click="probe(item.domain)">проверить</Button>
            <Button size="sm" variant="ghost" @click="removeDomain(item.domain)">
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">Локальные домены не настроены.</p>

      <form class="flex flex-col gap-2 sm:flex-row" @submit.prevent="addDomain">
        <div class="flex-1 space-y-1">
          <Label>Домен</Label>
          <Input v-model="domain" placeholder="app.localhost" autocomplete="off" />
        </div>
        <div class="w-full space-y-1 sm:w-28">
          <Label>Порт</Label>
          <Input v-model="port" placeholder="3000" inputmode="numeric" />
        </div>
        <Button type="submit" class="self-end">
          <Plus class="h-3.5 w-3.5" />
          Добавить
        </Button>
      </form>

      <div class="space-y-1 rounded-lg border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        <p>
          <code>*.localhost</code> браузеры резолвят сами — hosts не нужен.
          Для настоящих имён (например, <code>app.example.com</code>) нужна
          запись <code>127.0.0.1</code> в hosts: запустите
          <code>{{ lp().hostsScript || 'local-domains-hosts.bat' }}</code> от
          администратора.
        </p>
        <div class="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" @click="prepareHosts">
            <ShieldCheck class="h-3.5 w-3.5" />
            Подготовить скрипт hosts
          </Button>
          <Button size="sm" variant="outline" @click="editHosts">
            <Pencil class="h-3.5 w-3.5" />
            Редактировать hosts
          </Button>
        </div>
      </div>
    </div>
  </Card>
</template>
