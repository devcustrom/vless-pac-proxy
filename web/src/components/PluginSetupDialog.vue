<script setup>
import { computed, ref, watch } from 'vue'
import { Field, Form, reset, submit, useForm } from '@formisch/vue'
import { LockKeyhole } from 'lucide-vue-next'
import { api, installCerts, notify } from '@/lib/store'
import { useActionMutation } from '@/lib/queries'
import { plainDomainSchema } from '@/lib/validators'
import Dialog from '@/components/ui/Dialog.vue'
import Badge from '@/components/ui/Badge.vue'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import Label from '@/components/ui/Label.vue'

const props = defineProps({
  open: Boolean,
  firstRun: Boolean,
  s: { type: Object, required: true },
})
const emit = defineEmits(['close', 'enabled'])

const form = useForm({
  schema: plainDomainSchema,
  initialInput: { domain: 'panel.localhost' },
})
const certBusy = ref(false)

watch(() => props.open, (open) => {
  if (open) reset(form)
})

const enableAction = useActionMutation(
  (panelDomain) => api('/api/plugins', { localDomains: true, panelDomain }),
  [['status']],
)
const skipAction = useActionMutation(
  () => api('/api/onboarding', {}),
  [['status']],
)
const enabling = computed(() => enableAction.isLoading.value)
const skipping = computed(() => skipAction.isLoading.value)

async function install() {
  certBusy.value = true
  try {
    await installCerts()
  } finally {
    certBusy.value = false
  }
}

async function enable(output) {
  try {
    const result = await enableAction.mutateAsync(output.domain)
    await skipAction.mutateAsync().catch(() => {})
    notify('Плагин локальных доменов включён')
    if (result.httpsError) notify(result.httpsError, 'error')
    emit('enabled', result)
    emit('close')
  } catch {
    /* тост показан */
  }
}

async function skip() {
  if (props.firstRun) {
    await skipAction.mutateAsync().catch(() => {})
  }
  emit('close')
}
</script>

<template>
  <Dialog
    :open="open"
    :title="firstRun ? 'Первый запуск: локальные домены' : 'Включить плагин доменов'"
    :description="firstRun
      ? 'Можно включить плагин и задать красивый домен для панели. Это не обязательно.'
      : 'Плагин поднимает HTTP/HTTPS на loopback и даёт домены вида app.example.com.'"
    @close="emit('close')"
  >
    <Form :of="form" class="space-y-4" @submit="enable">
      <Field v-slot="field" :of="form" :path="['domain']">
        <div class="space-y-1">
          <Label>Домен панели</Label>
          <Input
            v-model="field.input"
            v-bind="field.props"
            placeholder="panel.localhost"
            autocomplete="off"
          />
          <p class="text-xs text-muted-foreground">
            Домен будет указывать на эту панель. Для настоящих имён понадобится
            запись в hosts — кнопка появится после включения.
          </p>
          <p v-if="field.errors" class="text-xs text-destructive">{{ field.errors[0] }}</p>
        </div>
      </Field>

      <div class="space-y-2 rounded-lg border bg-muted/30 px-3 py-2 text-xs">
        <div class="flex flex-wrap items-center gap-2">
          <LockKeyhole class="h-3.5 w-3.5" />
          <span>HTTPS без предупреждений требует доверия локальному CA.</span>
          <Badge :variant="s.caTrusted ? 'success' : 'warning'">
            {{ s.caTrusted ? 'CA доверенный' : 'CA не установлен' }}
          </Badge>
        </div>
        <p class="text-muted-foreground">
          Без сертификата HTTP-домены работают, а HTTPS браузер будет открывать
          с предупреждением. Установка запросит UAC/пароль.
        </p>
        <Button
          v-if="!s.caTrusted"
          type="button"
          size="sm"
          variant="outline"
          :loading="certBusy"
          @click="install"
        >
          <LockKeyhole class="h-3.5 w-3.5" />
          Установить сертификаты
        </Button>
      </div>
    </Form>

    <template #footer>
      <Button variant="outline" :loading="skipping" @click="skip">
        {{ firstRun ? 'Не сейчас' : 'Отмена' }}
      </Button>
      <Button :loading="enabling" @click="submit(form)">Включить плагин</Button>
    </template>
  </Dialog>
</template>
