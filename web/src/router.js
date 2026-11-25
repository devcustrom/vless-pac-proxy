import { createRouter, createWebHistory } from 'vue-router'

const routes = [
  { path: '/', redirect: '/overview' },
  {
    path: '/overview',
    name: 'overview',
    component: () => import('./views/OverviewView.vue'),
    meta: { title: 'Обзор' },
  },
  { path: '/configs', redirect: '/channels' },
  {
    path: '/channels',
    name: 'channels',
    component: () => import('./views/ChannelsView.vue'),
    meta: { title: 'Каналы' },
  },
  {
    path: '/sites',
    name: 'sites',
    component: () => import('./views/SitesView.vue'),
    meta: { title: 'Сайты' },
  },
  {
    path: '/routing',
    name: 'routing',
    component: () => import('./views/RoutingView.vue'),
    meta: { title: 'Маршрутизация' },
  },
  {
    path: '/telegram',
    name: 'telegram',
    component: () => import('./views/TelegramView.vue'),
    meta: { title: 'Telegram', plugin: 'telegram' },
  },
  {
    path: '/dpi',
    name: 'dpi',
    component: () => import('./views/DpiView.vue'),
    meta: { title: 'Обход DPI', plugin: 'zapret' },
  },
  {
    path: '/dpi/catalog',
    name: 'catalog',
    component: () => import('./views/CatalogView.vue'),
    meta: { title: 'Каталог Zaprett', plugin: 'zapret' },
  },
  { path: '/domains', redirect: '/domains/local' },
  {
    path: '/domains/local',
    name: 'domains-local',
    component: () => import('./views/LocalDomainsView.vue'),
    meta: { title: 'Локальные домены', plugin: 'localDomains' },
  },
  // Старая ссылка: маршрутизация больше не относится к плагину локальных доменов.
  { path: '/domains/routing', redirect: '/routing' },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('./views/SettingsView.vue'),
    meta: { title: 'Настройки' },
  },
  {
    path: '/docs',
    redirect: { path: '/overview', query: { docs: '1' } },
  },
  { path: '/:pathMatch(.*)*', redirect: '/overview' },
]

// Старые ссылки вида /#/settings продолжают работать: переносим hash-путь
// в обычный path до инициализации роутера.
function migrateLegacyHash() {
  if (!window.location.hash.startsWith('#/')) return
  const legacy = window.location.hash.slice(1)
  window.history.replaceState(null, '', `${window.location.pathname}${window.location.search}${legacy}`)
}

export function createAppRouter() {
  migrateLegacyHash()
  const router = createRouter({
    history: createWebHistory(import.meta.env.BASE_URL),
    routes,
    scrollBehavior() {
      return { top: 0 }
    },
  })

  router.afterEach((to) => {
    const title = to.meta?.title
    document.title = title ? `${title} · wireproxy-control` : 'wireproxy-control'
  })

  return router
}
