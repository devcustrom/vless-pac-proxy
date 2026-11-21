import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import App from './App.vue'
import { createAppRouter } from './router'
import './style.css'

const dark = window.matchMedia('(prefers-color-scheme: dark)')
const applyTheme = () => document.documentElement.classList.toggle('dark', dark.matches)
applyTheme()
dark.addEventListener?.('change', applyTheme)

const app = createApp(App)
app.use(createPinia())
app.use(createAppRouter())
app.use(PiniaColada, {
  queryOptions: { staleTime: 2000 },
})
app.mount('#app')
