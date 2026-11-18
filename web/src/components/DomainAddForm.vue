<script setup>
import { Form, Field, reset, useForm } from '@formisch/vue'
import { Plus } from 'lucide-vue-next'
import Button from '@/components/ui/Button.vue'
import Input from '@/components/ui/Input.vue'
import { digitsOnly, domainPortSchema, plainDomainSchema } from '@/lib/validators'

const props = defineProps({
  showPort: { type: Boolean, default: false },
  submit: { type: Function, required: true },
  loading: Boolean,
  disabled: Boolean,
  placeholder: { type: String, default: 'example.com' },
})

const form = useForm({
  schema: props.showPort ? domainPortSchema : plainDomainSchema,
  initialInput: props.showPort ? { domain: '', port: '' } : { domain: '' },
})

async function onSubmit(output) {
  const result = await props.submit(output)
  if (result !== false) reset(form)
}
</script>

<template>
  <Form :of="form" class="flex flex-col gap-2 sm:flex-row" @submit="onSubmit">
    <Field v-slot="field" :of="form" :path="['domain']">
      <div class="min-w-0 flex-1">
        <Input
          v-model="field.input"
          v-bind="field.props"
          :placeholder="placeholder"
          :disabled="disabled"
          autocomplete="off"
        />
        <p v-if="field.errors" class="mt-1 text-xs text-destructive">{{ field.errors[0] }}</p>
      </div>
    </Field>
    <Field v-if="showPort" v-slot="field" :of="form" :path="['port']">
      <div class="w-full sm:w-28">
        <Input
          v-model="field.input"
          v-bind="field.props"
          placeholder="3000"
          inputmode="numeric"
          maxlength="5"
          :disabled="disabled"
          @input="field.input = digitsOnly(field.input)"
        />
        <p v-if="field.errors" class="mt-1 text-xs text-destructive">{{ field.errors[0] }}</p>
      </div>
    </Field>
    <Button type="submit" :loading="loading" :disabled="disabled" class="self-start sm:self-auto">
      <Plus class="h-3.5 w-3.5" />
      Добавить
    </Button>
  </Form>
</template>
