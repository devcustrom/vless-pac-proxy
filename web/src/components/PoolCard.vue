<script setup>
import { computed, ref, watch } from 'vue'
import { Gauge, Plus, RefreshCw, Trash2, Zap } from 'lucide-vue-next'
import { api, fmtTime, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import Select from '@/components/ui/Select.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const subscription = ref('')
const working = computed(() => (props.s.pool || []).filter((item) => item.ok))
const active = computed(() => (props.s.pool || []).find((item) => item.active))
const autoPool = ref((props.s.autoPoolRefreshHours || 0) > 0)
const hours = ref(String(props.s.autoPoolRefreshHours || 1))
const hourOptions = [
  { value: '1', label: 'каждый час' },
  { value: '3', label: 'каждые 3 ч' },
  { value: '6', label: 'каждые 6 ч' },
  { value: '12', label: 'каждые 12 ч' },
  { value: '24', label: 'раз в сутки' },
]

const refreshAction = useActionMutation(
  () => api('/api/pool/refresh', {}),
  [['status']],
)
const importAction = useActionMutation(
  (groups) => api('/api/subscriptions', { action: 'import-igareck', groups }),
  [['status']],
)
const useAction = useActionMutation(
  (index) => api('/api/pool/use', { index }),
  [['status']],
)
const subAction = useActionMutation(
  ({ action, url }) => api('/api/subscriptions', { action, url }),
  [['status']],
)
const autoAction = useActionMutation(
  (payload) => api('/api/auto', payload),
  [['status']],
)
const refreshLoading = computed(() => refreshAction.isLoading.value)
const importLoading = computed(() => importAction.isLoading.value)
const useLoading = computed(() => useAction.isLoading.value)
const subBusy = ref('')
const autoBusy = ref(false)
const rotateBusy = ref(false)
const usingIndex = ref(null)
watch(
  () => props.s.autoPoolRefreshHours,
  (value) => {
    if (autoBusy.value) return
    autoPool.value = (value || 0) > 0
    hours.value = String(value || 1)
  },
)

async function addSubscription() {
  const url = subscription.value.trim()
  if (!url) return
  subBusy.value = 'add'
  try {
    await subAction.mutateAsync({ action: 'add', url })
    subscription.value = ''
    notify('Подписка добавлена')
  } catch {
    /* тост уже показан */
  } finally {
    subBusy.value = ''
  }
}

async function removeSubscription(url) {
  if (!confirm('Удалить подписку? Конфиги, уже добавленные в пул, останутся.')) return
  subBusy.value = url
  try {
    await subAction.mutateAsync({ action: 'remove', url })
    notify('Подписка удалена')
  } catch {
    /* тост уже показан */
  } finally {
    subBusy.value = ''
  }
}

async function refreshPool() {
  try {
    await refreshAction.mutateAsync()
    notify('Проверка конфигов запущена')
  } catch {
    /* тост уже показан */
  }
}

async function importIgareck(groups) {
  try {
    const result = await importAction.mutateAsync(groups)
    notify(`Подписки igareck добавлены (${result.added ?? 0})`)
  } catch {
    /* тост уже показан */
  }
}

async function useConfig(item) {
  usingIndex.value = item.i
  try {
    await useAction.mutateAsync(item.i)
    notify('Конфиг применён: ' + item.name)
  } catch {
    /* тост уже показан */
  } finally {
    usingIndex.value = null
  }
}

async function saveAuto() {
  autoBusy.value = true
  try {
    await autoAction.mutateAsync({
      autoPoolRefreshHours: autoPool.value ? Number(hours.value) : 0,
    })
    notify('Настройки обновления сохранены')
  } catch {
    autoPool.value = (props.s.autoPoolRefreshHours || 0) > 0
    hours.value = String(props.s.autoPoolRefreshHours || 1)
    /* тост уже показан */
  } finally {
    autoBusy.value = false
  }
}

async function toggleRotate(value) {
  rotateBusy.value = true
  try {
    await autoAction.mutateAsync({ autoRotate: value })
    notify(value ? 'Автопереключение включено' : 'Автопереключение выключено')
  } catch {
    /* тост уже показан */
  } finally {
    rotateBusy.value = false
  }
}
</script>

<template>
  <Card
    title="Пул конфигов"
    description="Список строится по живому трафику: рабочие конфиги перепроверяются каждые 5 минут, а перед применением выбранный проверяется ещё раз."
  >
    <template #icon>
      <Gauge class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-4">
      <div class="space-y-2">
        <Label>Подписки</Label>
        <ul v-if="s.subscriptions?.length" class="divide-y rounded-lg border">
          <li
            v-for="url in s.subscriptions"
            :key="url"
            class="flex items-center gap-2 px-3 py-2"
          >
            <span class="min-w-0 flex-1 truncate text-xs text-muted-foreground" :title="url">
              {{ url }}
            </span>
            <Button
              size="sm"
              variant="ghost"
              :loading="subBusy === url"
              :disabled="!!subBusy"
              @click="removeSubscription(url)"
            >
              <Trash2 class="h-3.5 w-3.5" />
            </Button>
          </li>
        </ul>
        <form class="flex gap-2" @submit.prevent="addSubscription">
          <Input
            v-model="subscription"
            placeholder="https://.../subscription.txt"
            autocomplete="off"
          />
          <Button
            type="submit"
            variant="outline"
            size="sm"
            :loading="subBusy === 'add'"
            :disabled="!!subBusy || !subscription.trim()"
          >
            <Plus class="h-3.5 w-3.5" />
            Добавить
          </Button>
        </form>
      </div>

      <div class="flex flex-wrap items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2 text-xs">
        <a
          href="https://github.com/igareck/vpn-configs-for-russia"
          target="_blank"
          class="text-primary underline underline-offset-4"
        >
          igareck/vpn-configs-for-russia
        </a>
        <span class="text-muted-foreground">BLACK/WHITE-списки</span>
        <Button size="sm" variant="outline" :loading="importLoading" @click="importIgareck(['black'])">
          BLACK
        </Button>
        <Button size="sm" variant="outline" :loading="importLoading" @click="importIgareck(['white'])">
          WHITE (CIDR/SNI)
        </Button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          :disabled="s.poolTesting"
          :loading="refreshLoading"
          @click="refreshPool"
        >
          <RefreshCw class="h-3.5 w-3.5" />
          {{ s.poolTesting ? 'Проверяю…' : 'Проверить конфиги' }}
        </Button>
        <span class="text-xs text-muted-foreground">
          <template v-if="s.poolUpdatedAt">
            рабочих {{ working.length }} из {{ (s.pool || []).length }} · {{ fmtTime(s.poolUpdatedAt) }}
          </template>
          <template v-else-if="s.poolError">ошибка: {{ s.poolError }}</template>
          <template v-else>ещё не проверялись</template>
        </span>
      </div>

      <div class="flex flex-wrap items-center gap-4 rounded-lg border bg-muted/30 px-3 py-2">
        <label class="flex items-center gap-2 text-sm">
          <Switch
            :model-value="autoPool"
            :disabled="autoBusy || rotateBusy"
            @update:model-value="(v) => { autoPool = v; saveAuto() }"
          />
          Автообновление
        </label>
        <div class="w-36">
          <Select
            v-model="hours"
            :options="hourOptions"
            :disabled="autoBusy || rotateBusy || !autoPool"
            @update:model-value="autoPool && saveAuto()"
          />
        </div>
        <label class="flex items-center gap-2 text-sm">
          <Switch
            :model-value="!!s.autoRotate"
            :disabled="rotateBusy || autoBusy"
            @update:model-value="toggleRotate"
          />
          Автопереключение
        </label>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>Рабочие конфиги</Label>
          <Badge v-if="working.length" variant="success">{{ working.length }}</Badge>
        </div>
        <div v-if="active" class="rounded-lg border bg-success/5 px-3 py-2 text-xs">
          <Badge variant="success">активен</Badge>
          <span class="ml-2">{{ active.name || active.address }}</span>
        </div>
        <ul v-if="working.length" class="max-h-64 divide-y overflow-auto rounded-lg border scrollbar-thin">
          <li
            v-for="item in working.slice(0, 25)"
            :key="item.i"
            class="flex items-center gap-2 px-3 py-2 text-sm"
            :class="{ 'bg-success/5': item.active }"
          >
            <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-success" />
            <span class="min-w-0 flex-1 truncate">
              {{ item.name || item.address }}
              <span class="text-xs text-muted-foreground">
                {{ item.protocol }} {{ item.address }}:{{ item.port }}
                <template v-if="item.ms">· {{ item.ms }} мс</template>
              </span>
            </span>
            <Badge v-if="item.active" variant="success">активен</Badge>
            <Button
              v-else
              size="sm"
              variant="outline"
              :loading="usingIndex === item.i"
              :disabled="useLoading"
              @click="useConfig(item)"
            >
              <Zap class="h-3.5 w-3.5" />
              Использовать
            </Button>
          </li>
        </ul>
        <p v-else class="text-xs text-muted-foreground">
          {{ (s.pool || []).length ? 'Рабочих конфигов нет — запустите проверку заново.' : 'Пул ещё не проверялся.' }}
        </p>
      </div>
    </div>
  </Card>
</template>
