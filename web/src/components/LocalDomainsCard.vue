<script setup>
import { computed, reactive, ref } from 'vue'
import { ExternalLink, Globe, LockKeyhole, Pencil, ShieldCheck, Trash2 } from 'lucide-vue-next'
import { api, installCerts, notify, post } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import DomainAddForm from '@/components/DomainAddForm.vue'

const props = defineProps({ s: { type: Object, required: true } })

const probes = reactive({})
const certBusy = ref(false)
const applyBusy = ref(false)
const editBusy = ref(false)
const removing = ref('')
const probing = ref('')
const addAction = useActionMutation(
  async (input) => api('/api/local-domains', { action: 'add', ...input }),
  [['status']],
)
const addLoading = computed(() => addAction.isLoading.value)

async function install() {
  certBusy.value = true
  try {
    await installCerts()
  } finally {
    certBusy.value = false
  }
}

async function applyHosts() {
  applyBusy.value = true
  try {
    await api('/api/local-domains/hosts/apply', {})
    notify('Подтвердите UAC/пароль — hosts будут обновлены')
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    applyBusy.value = false
  }
}

async function editHosts() {
  editBusy.value = true
  try {
    await api('/api/local-domains/hosts/edit', {})
    notify('Открыт редактор hosts (подтвердите UAC/пароль)')
  } catch (error) {
    notify(error.message, 'error')
  } finally {
    editBusy.value = false
  }
}

const lp = () => props.s.localProxy || {}
const httpPort = () => (lp().port === 80 ? '' : `:${lp().port}`)
const httpsPort = () => (lp().httpsPort === 443 ? '' : `:${lp().httpsPort}`)
const isMac = () => String(lp().hostsScript || '').endsWith('.command')

async function addDomain(output) {
  try {
    await addAction.mutateAsync({ domain: output.domain, port: output.port })
    notify(`Домен добавлен: http://${output.domain}${httpPort()}`)
  } catch {
    /* тост уже показан */
  }
}

async function removeDomain(name) {
  removing.value = name
  try {
    await post('/api/local-domains', { action: 'remove', domain: name })
    delete probes[name]
    notify('Домен убран')
  } catch {
    /* тост уже показан */
  } finally {
    removing.value = ''
  }
}

async function probe(name) {
  probing.value = name
  try {
    const result = await api('/api/local-domains/probe', { domain: name })
    probes[name] = result
  } catch (error) {
    probes[name] = { status: 0, error: error.message }
  } finally {
    probing.value = ''
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
        <Button
          v-if="!s.caTrusted"
          size="sm"
          variant="outline"
          :loading="certBusy"
          @click="install"
        >
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
                  http://{{ item.domain }}{{ httpPort() }}
                  <ExternalLink class="h-3 w-3" />
                </a>
                <a
                  :href="`https://${item.domain}${httpsPort()}`"
                  target="_blank"
                  class="text-xs text-muted-foreground hover:underline"
                >
                  https://{{ item.domain }}{{ httpsPort() }}
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
            <Button
              size="sm"
              variant="outline"
              :loading="probing === item.domain"
              :disabled="!!probing || !!removing"
              @click="probe(item.domain)"
            >
              проверить
            </Button>
            <Button
              size="sm"
              variant="ghost"
              :loading="removing === item.domain"
              :disabled="!!probing || !!removing"
              @click="removeDomain(item.domain)"
            >
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </div>
        </li>
      </ul>
      <p v-else class="text-xs text-muted-foreground">Локальные домены не настроены.</p>

      <DomainAddForm
        show-port
        placeholder="app.example.com"
        :submit="addDomain"
        :loading="addLoading"
      />

      <div class="space-y-1 rounded-lg border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        <p>
          <code>*.localhost</code> браузеры резолвят сами — hosts не нужен.
          Для настоящих имён (например, <code>app.example.com</code>) нужна
          запись <code>127.0.0.1</code> в hosts: нажмите «Применить hosts» и
          подтвердите запрос системы. Всё делается из панели, без терминала.
        </p>
        <div class="flex flex-wrap gap-2">
          <Button size="sm" :loading="applyBusy" @click="applyHosts">
            <ShieldCheck class="h-3.5 w-3.5" />
            Применить hosts
          </Button>
          <Button
            v-if="!isMac()"
            size="sm"
            variant="outline"
            :loading="editBusy"
            @click="editHosts"
          >
            <Pencil class="h-3.5 w-3.5" />
            Редактировать hosts
          </Button>
        </div>
      </div>
    </div>
  </Card>
</template>
