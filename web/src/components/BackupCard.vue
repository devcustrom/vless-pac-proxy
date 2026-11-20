<script setup>
import { ref } from 'vue'
import { Download, Upload } from 'lucide-vue-next'
import { notify } from '@/lib/store'
import Card from '@/components/ui/Card.vue'
import Button from '@/components/ui/Button.vue'

const fileInput = ref(null)
const restoring = ref(false)

function pickBackup() {
  fileInput.value?.click()
}

async function restore(event) {
  const file = event.target.files?.[0]
  event.target.value = ''
  if (!file) return
  if (
    !confirm(
      'Восстановить данные из бэкапа? Настройки, конфиги, сертификаты и история будут заменены. Менеджер перезапустится.',
    )
  ) {
    return
  }
  restoring.value = true
  try {
    const body = await file.arrayBuffer()
    const response = await fetch('/api/backup/restore', {
      method: 'POST',
      headers: { 'Content-Type': 'application/zip', 'X-WPC': '1' },
      body,
    })
    const data = await response.json().catch(() => ({}))
    if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`)
    notify('Бэкап восстановлен, менеджер перезапускается…')
    setTimeout(() => window.location.reload(), 6000)
  } catch (error) {
    restoring.value = false
    notify(error.message, 'error')
  }
}
</script>

<template>
  <Card
    title="Резервная копия"
    description="Настройки, конфиги, списки, сертификаты и история сайтов в одном zip."
  >
    <template #icon>
      <Download class="h-4 w-4 text-primary" />
    </template>

    <div class="space-y-3">
      <p class="text-xs text-muted-foreground">
        Бэкап содержит приватные ключи, секрет Telegram и сертификаты. Храните
        архив в защищённом месте.
      </p>
      <div class="flex flex-wrap gap-2">
        <a
          href="/api/backup"
          class="inline-flex h-8 items-center gap-2 rounded-md border border-input bg-background px-3 text-xs font-medium hover:bg-accent"
        >
          <Download class="h-3.5 w-3.5" />
          Скачать бэкап
        </a>
        <Button size="sm" variant="outline" :loading="restoring" @click="pickBackup">
          <Upload class="h-3.5 w-3.5" />
          Восстановить
        </Button>
        <input
          ref="fileInput"
          type="file"
          accept=".zip,application/zip"
          class="hidden"
          @change="restore"
        />
      </div>
      <p class="text-xs text-muted-foreground">
        Восстановление заменяет данные и перезапускает менеджер; бинарники и
        движки не затрагиваются.
      </p>
    </div>
  </Card>
</template>
