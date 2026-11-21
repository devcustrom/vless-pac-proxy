<script setup>
import { computed, ref } from 'vue'
import { Search, Wand2 } from 'lucide-vue-next'
import { RouterLink } from 'vue-router'
import { api, notify } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import StrategyDialog from '@/components/StrategyDialog.vue'

const props = defineProps({ s: { type: Object, required: true } })

const domain = ref('')
const result = ref(null)
const busy = ref(false)
const strategyFor = ref('')
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

async function check() {
  const value = domain.value.trim()
  if (!value) return
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
  <Card title="Проверить домен" description="Быстрая проверка напрямую, через VPN и Zapret.">
    <template #icon>
      <Search class="h-4 w-4 text-primary" />
    </template>
    <form class="flex gap-2" @submit.prevent="check">
      <Input v-model="domain" placeholder="example.com" autocomplete="off" />
      <Button type="submit" :loading="busy">
        <Search class="h-3.5 w-3.5" />
        Проверить
      </Button>
    </form>
    <div v-if="result" class="mt-3 space-y-2 rounded-lg border bg-muted/30 px-3 py-2 text-sm">
      <div class="flex flex-wrap items-center gap-1.5">
        <Badge :variant="codeVariant(result.direct)">напрямую {{ codeText(result.direct) }}</Badge>
        <Badge :variant="codeVariant(result.vpn)">VPN {{ codeText(result.vpn) }}</Badge>
        <Badge :variant="result.zapretRunning ? codeVariant(result.zapret) : 'secondary'">
          Zapret {{ result.zapretRunning ? codeText(result.zapret) : 'выкл' }}
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
          v-if="canGenerate && ['000', '403', '451'].includes(result.direct)"
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
