<script setup lang="ts">
// 右侧主机栏：左键选中 / 双击直连 / 右键菜单（连接、编辑、Ping、删除）
// 含搜索栏；主机列表按分组自动组织（分组标题 + 组内主机）
// 语言切换已移至左侧底部状态栏（用户反馈：主机栏与底栏分离，底栏后续放设置图标等功能）
import { computed, ref } from 'vue'
import { t } from '../composables/i18n'

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
  save_password: boolean
  created_at: string
  updated_at: string
}

const props = defineProps<{
  hosts: Host[]
  searchQuery: string
}>()

const emit = defineEmits<{
  (e: 'connect', host: Host): void
  (e: 'edit', host: Host): void
  (e: 'ping', host: Host): void
  (e: 'delete', host: Host): void
  (e: 'new'): void
  (e: 'search', query: string): void
}>()

const selectedId = ref<string | null>(null)
const menu = ref<{ x: number; y: number; host: Host } | null>(null)

// 按分组组织主机列表：[分组名, 组内主机] 数组；空分组名归入"未分组"标题
const groupedHosts = computed(() => {
  const groups = new Map<string, Host[]>()
  for (const host of props.hosts) {
    const key = host.group_name || ''
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(host)
  }
  return Array.from(groups.entries())
})

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
    <div class="host-list">
      <template v-for="[groupName, groupHosts] in groupedHosts" :key="groupName || '__unassigned__'">
        <div class="group-header">{{ groupName || t('hosts.groupUnassigned') }}</div>
        <ul>
          <li v-for="host in groupHosts" :key="host.id"
            :class="{ active: selectedId === host.id }"
            @click="selectedId = host.id"
            @dblclick="emit('connect', host)"
            @contextmenu="onContextMenu($event, host)">
            <span class="host-name">{{ host.name }}</span>
            <span class="host-addr">{{ host.address }}:{{ host.port }}</span>
          </li>
        </ul>
      </template>
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
/* 主机栏整体：右侧区域内的上半部分（flex:1 上下贯通）；左右分隔线由 right-area 的 border-left 承担 */
.host-panel { width: 220px; min-width: 220px; background: var(--color-background-soft); display: flex; flex-direction: column; flex: 1; min-height: 0; }
.sidebar-header { display: flex; justify-content: space-between; align-items: center; padding: 0.6rem 0.8rem; border-bottom: 1px solid var(--color-border); }
.sidebar-header h2 { font-size: 0.95rem; color: var(--color-heading); }
.search-bar { padding: 0.4rem 0.6rem; border-bottom: 1px solid var(--color-border); }
.search-bar input { width: 100%; padding: 0.3rem 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background); color: var(--color-text); font-size: 0.8rem; box-sizing: border-box; }
.host-list { flex: 1; overflow-y: auto; list-style: none; padding: 0; margin: 0; }
/* 分组标题：小号灰色，与主机项区分 */
.group-header { padding: 0.35rem 0.8rem 0.15rem; font-size: 0.7rem; font-weight: 600; color: var(--color-text); opacity: 0.55; text-transform: uppercase; }
.host-list ul { list-style: none; padding: 0; margin: 0; }
.host-list ul li { padding: 0.5rem 0.8rem; cursor: pointer; border-bottom: 1px solid var(--color-border); }
.host-list ul li:hover { background: var(--color-background-mute); }
.host-list ul li.active { background: var(--color-border-hover); }
.host-name { display: block; font-weight: 600; color: var(--color-heading); font-size: 0.85rem; }
.host-addr { font-size: 0.7rem; color: var(--color-text); opacity: 0.7; }

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
