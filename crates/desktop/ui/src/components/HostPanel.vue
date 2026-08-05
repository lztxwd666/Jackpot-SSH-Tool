<script setup lang="ts">
// 右侧主机栏：左键选中 / 双击直连 / 右键菜单（连接、编辑、Ping、删除）
// 含搜索栏与语言切换页脚（沿用原 App.vue 布局）
import { ref } from 'vue'
import { t, type Locale } from '../composables/i18n'

// 与后端 list_hosts 返回的 Host 结构一致（前端 host 列表即为完整 Host，不做收窄）
export interface Host {
  id: string
  name: string
  address: string
  port: number
  username: string
  auth_type: string
  group_name: string
  favorite: boolean
  notes: string
  created_at: string
  updated_at: string
}

const props = defineProps<{
  hosts: Host[]
  searchQuery: string
  locale: Locale
}>()

const emit = defineEmits<{
  (e: 'connect', host: Host): void
  (e: 'edit', host: Host): void
  (e: 'ping', host: Host): void
  (e: 'delete', host: Host): void
  (e: 'new'): void
  (e: 'search', query: string): void
  (e: 'locale-change', locale: Locale): void
}>()

const selectedId = ref<string | null>(null)
const menu = ref<{ x: number; y: number; host: Host } | null>(null)

function onContextMenu(e: MouseEvent, host: Host) {
  e.preventDefault()
  menu.value = { x: e.clientX, y: e.clientY, host }
}
function closeMenu() { menu.value = null }
function pick(action: 'connect' | 'edit' | 'ping' | 'delete') {
  if (!menu.value) return
  const host = menu.value.host
  closeMenu()
  // 逐个 emit：让 TS 精确匹配 defineEmits 的重载签名
  if (action === 'connect') emit('connect', host)
  else if (action === 'edit') emit('edit', host)
  else if (action === 'ping') emit('ping', host)
  else emit('delete', host)
}
function onSearchInput(e: Event) {
  emit('search', (e.target as HTMLInputElement).value)
}
function onLocaleChange(e: Event) {
  emit('locale-change', (e.target as HTMLSelectElement).value as Locale)
}
</script>

<template>
  <div class="host-panel" @click="closeMenu">
    <div class="sidebar-header">
      <h2>{{ t('hosts.title') }}</h2>
      <button class="btn btn-primary" @click="emit('new')">{{ t('hosts.add') }}</button>
    </div>
    <div class="search-bar">
      <input :value="searchQuery" type="text" :placeholder="t('hosts.searchPlaceholder')" @input="onSearchInput" />
    </div>
    <ul class="host-list">
      <li v-for="host in hosts" :key="host.id"
        :class="{ active: selectedId === host.id }"
        @click="selectedId = host.id"
        @dblclick="emit('connect', host)"
        @contextmenu="onContextMenu($event, host)">
        <span class="host-name">{{ host.name }}</span>
        <span class="host-addr">{{ host.address }}:{{ host.port }}</span>
      </li>
    </ul>
    <div class="sidebar-footer">
      <select class="locale-select" :value="locale" @change="onLocaleChange">
        <option value="en">English</option>
        <option value="zh">中文</option>
      </select>
    </div>
    <div v-if="menu" class="context-menu" :style="{ left: menu.x + 'px', top: menu.y + 'px' }">
      <div class="menu-item" @click="pick('connect')">{{ t('common.connect') }}</div>
      <div class="menu-item" @click="pick('edit')">{{ t('common.edit') }}</div>
      <div class="menu-item" @click="pick('ping')">{{ t('common.ping') }}</div>
      <div class="menu-item danger" @click="pick('delete')">{{ t('common.delete') }}</div>
    </div>
  </div>
</template>

<style scoped>
/* 主机栏整体：沿用原 App.vue 的 sidebar 布局样式 */
.host-panel { width: 220px; min-width: 220px; background: var(--color-background-soft); border-left: 1px solid var(--color-border); display: flex; flex-direction: column; }
.sidebar-header { display: flex; justify-content: space-between; align-items: center; padding: 0.6rem 0.8rem; border-bottom: 1px solid var(--color-border); }
.sidebar-header h2 { font-size: 0.95rem; color: var(--color-heading); }
.search-bar { padding: 0.4rem 0.6rem; border-bottom: 1px solid var(--color-border); }
.search-bar input { width: 100%; padding: 0.3rem 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background); color: var(--color-text); font-size: 0.8rem; box-sizing: border-box; }
.host-list { flex: 1; overflow-y: auto; list-style: none; padding: 0; margin: 0; }
.host-list li { padding: 0.5rem 0.8rem; cursor: pointer; border-bottom: 1px solid var(--color-border); }
.host-list li:hover { background: var(--color-background-mute); }
.host-list li.active { background: var(--color-border-hover); }
.host-name { display: block; font-weight: 600; color: var(--color-heading); font-size: 0.85rem; }
.host-addr { font-size: 0.7rem; color: var(--color-text); opacity: 0.7; }
.sidebar-footer { padding: 0.4rem 0.6rem; border-top: 1px solid var(--color-border); display: flex; align-items: center; justify-content: flex-end; }
.locale-select { background: var(--color-background); color: var(--color-text); border: 1px solid var(--color-border); border-radius: 4px; font-size: 0.7rem; padding: 0.1rem 0.2rem; cursor: pointer; }

/* context-menu 沿用 RemoteFileTree 样式；删除项红色警示 */
.context-menu {
  position: fixed; background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 4px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); z-index: 1000; min-width: 120px;
}
.menu-item { padding: 0.3rem 0.8rem; cursor: pointer; font-size: 0.8rem; }
.menu-item:hover { background: var(--color-background-mute); }
.menu-item.danger { color: #e5534b; }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn-primary { background: hsla(160, 100%, 37%, 1); color: #fff; border-color: hsla(160, 100%, 37%, 1); }
.btn-primary:hover { background: hsla(160, 100%, 30%, 1); }
</style>
