import { createApp } from 'vue'
import App from './App.vue'
import './style.css'

const dark = window.matchMedia('(prefers-color-scheme: dark)')
const applyTheme = () => document.documentElement.classList.toggle('dark', dark.matches)
applyTheme()
dark.addEventListener?.('change', applyTheme)

createApp(App).mount('#app')
