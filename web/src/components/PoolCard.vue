<script setup>
import { computed, ref } from 'vue'
import { Gauge, Plus, RefreshCw, Trash2, Zap } from 'lucide-vue-next'
import { fmtTime, notify, post } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'
import Switch from '@/components/ui/Switch.vue'

const props = defineProps({ s: { type: Object, required: true } })

const subscription = ref('')
const busy = ref(false)
const importing = ref(false)

const working = computed(() => (props.s.pool || []).filter((item) => item.ok))
const autoPool = ref((props.s.autoPoolRefreshHours || 0) > 0)
const hours = ref(String(props.s.autoPoolRefreshHours || 1))

async function addSubscription() {
  const url = subscription.value.trim()
  if (!url) return
  try {
    await post('/api/subscriptions', { action: 'add', url })
    subscription.value = ''
    notify('Подписка добавлена')
  } catch {
    /* тост уже показан */
  }
}

async function removeSubscription(url) {
  try {
    await post('/api/subscriptions', { action: 'remove', url })
    notify('Подписка удалена')
  } catch {
    /* тост уже показан */
  }
}

async function refreshPool() {
  busy.value = true
  try {
    await post('/api/pool/refresh')
    notify('Проверка конфигов запущена')
  } catch {
    /* тост уже показан */
  } finally {
    busy.value = false
  }
}

async function importIgareck() {
  importing.value = true
  try {
    const result = await post('/api/subscriptions', { action: 'import-igareck' })
    notify(`Подписки igareck добавлены (${result.subscriptions?.length || 0})`)
  } catch {
    /* тост уже показан */
  } finally {
    importing.value = false
  }
}

async function useConfig(item) {
  try {
    await post('/api/pool/use', { index: item.i })
    notify('Конфиг применён: ' + item.name)
  } catch {
    /* тост уже показан */
  }
}

async function saveAuto() {
  try {
    await post('/api/auto', {
      autoPoolRefreshHours: autoPool.value ? Number(hours.value) : 0,
      autoRotate: props.s.autoRotate,
    })
    notify('Настройки обновления сохранены')
  } catch {
    /* тост уже показан */
  }
}

async function toggleRotate(value) {
  try {
    await post('/api/auto', { autoRotate: value })
    notify(value ? 'Автопереключение включено' : 'Автопереключение выключено')
  } catch {
    /* тост уже показан */
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
            <Button size="sm" variant="ghost" @click="removeSubscription(url)">
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
          <Button type="submit" variant="outline" size="sm">
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
        <Button size="sm" variant="outline" :disabled="importing" @click="importIgareck">
          Импортировать конфиги
        </Button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <Button size="sm" :disabled="s.poolTesting || busy" @click="refreshPool">
          <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': s.poolTesting }" />
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
          <Switch :model-value="autoPool" @update:model-value="(v) => { autoPool = v; saveAuto() }" />
          Автообновление
        </label>
        <select
          v-model="hours"
          class="h-8 rounded-md border border-input bg-background px-2 text-sm"
          @change="autoPool && saveAuto()"
        >
          <option value="1">каждый час</option>
          <option value="3">каждые 3 ч</option>
          <option value="6">каждые 6 ч</option>
          <option value="12">каждые 12 ч</option>
          <option value="24">раз в сутки</option>
        </select>
        <label class="flex items-center gap-2 text-sm">
          <Switch :model-value="!!s.autoRotate" @update:model-value="toggleRotate" />
          Автопереключение
        </label>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <Label>Рабочие конфиги</Label>
          <Badge v-if="working.length" variant="success">{{ working.length }}</Badge>
        </div>
        <ul v-if="working.length" class="max-h-64 divide-y overflow-auto rounded-lg border scrollbar-thin">
          <li
            v-for="item in working.slice(0, 25)"
            :key="item.i"
            class="flex items-center gap-2 px-3 py-2 text-sm"
          >
            <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-success" />
            <span class="min-w-0 flex-1 truncate">
              {{ item.name || item.address }}
              <span class="text-xs text-muted-foreground">
                {{ item.protocol }} {{ item.address }}:{{ item.port }}
                <template v-if="item.ms">· {{ item.ms }} мс</template>
              </span>
            </span>
            <Button size="sm" variant="outline" @click="useConfig(item)">
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
