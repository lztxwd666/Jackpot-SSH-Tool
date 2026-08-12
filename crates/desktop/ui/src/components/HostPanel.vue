<script setup lang="ts">
// 右侧主机栏：左键选中 / 双击直连 / 右键菜单（连接、编辑、Ping、删除）
// 含搜索栏；主机列表按分组自动组织（分组标题 + 组内主机）
// 语言切换已移至左侧底部状态栏（用户反馈：主机栏与底栏分离，底栏后续放设置图标等功能）
import { computed, onBeforeUnmount, ref } from 'vue'
import { clampFloatPos } from '../composables/pos'
import { useClickOutsideClose } from '../composables/menu'
import { useClearSelectionOnOutside } from '../composables/selection'
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
  (e: 'toggle-favorite', host: Host): void
  (e: 'new'): void
  (e: 'search', query: string): void
}>()

const selectedId = ref<string | null>(null)
const menu = ref<{ x: number; y: number; host: Host } | null>(null)

// 悬停信息卡：鼠标悬停主机项显示详情（参考 Tabby/WindTerm 等成熟客户端：名称/地址/用户名/认证/分组/备注）
// 位置在悬停起始处固定（不随鼠标移动，避免抖动）；clamp 防溢出窗口边缘（主机栏在右，卡默认偏右下）
const TIP_WIDTH = 200
const TIP_HEIGHT = 150
// 悬停延迟：光标停留 300ms 才显示信息卡（快速扫过主机列表不触发，成熟 UI 惯例）
const TIP_DELAY = 300
const tip = ref<{ x: number; y: number; host: Host } | null>(null)
let tipTimer: number | undefined
function showTip(e: MouseEvent, host: Host) {
  // 右键菜单打开时不显示悬停卡（两浮层不重叠）
  if (menu.value) return
  if (tipTimer !== undefined) window.clearTimeout(tipTimer)
  tipTimer = window.setTimeout(() => {
    const p = clampFloatPos(e.clientX + 12, e.clientY + 12, TIP_WIDTH, TIP_HEIGHT)
    tip.value = { x: p.x, y: p.y, host }
  }, TIP_DELAY)
}
function hideTip() {
  if (tipTimer !== undefined) {
    window.clearTimeout(tipTimer)
    tipTimer = undefined
  }
  tip.value = null
}
// 组件卸载时清理悬停定时器（防泄漏：卸载后定时器不再有实际作用）
onBeforeUnmount(() => {
  if (tipTimer !== undefined) window.clearTimeout(tipTimer)
})

// 右键菜单位置 clamp（估算宽高，防贴窗口右缘溢出被裁剪）
const menuStyle = computed(() => {
  if (!menu.value) return { left: '0px', top: '0px' }
  const p = clampFloatPos(menu.value.x, menu.value.y, 140, 140)
  return { left: p.x + 'px', top: p.y + 'px' }
})

// 认证方式显示名（与主机表单下拉文案一致；未知类型保留原值）
function authLabel(authType: string): string {
  if (authType === 'password') return t('form.authPassword')
  if (authType === 'private_key') return t('form.authPrivateKey')
  if (authType === 'agent') return t('form.authAgent')
  return authType
}

// 按分组组织主机列表：[分组名, 组内主机] 数组；空分组名归入"未分组"标题
// 组内收藏主机置顶（成熟客户端惯例：收藏优先展示，与星标视觉呼应）
const groupedHosts = computed(() => {
  const groups = new Map<string, Host[]>()
  for (const host of props.hosts) {
    const key = host.group_name || ''
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(host)
  }
  for (const arr of groups.values()) {
    arr.sort((a, b) => Number(b.favorite) - Number(a.favorite))
  }
  return Array.from(groups.entries())
})

function onContextMenu(e: MouseEvent, host: Host) {
  e.preventDefault()
  // 右键打开菜单时关闭悬停信息卡，避免两浮层重叠
  hideTip()
  menu.value = { x: e.clientX, y: e.clientY, host }
}
function closeMenu() { menu.value = null }
// 菜单打开时注册全局点击关闭（点击菜单外任意处消除菜单，标准交互）
useClickOutsideClose(menu, closeMenu)
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

// 面板点击：关闭菜单；空白区域（未命中主机行）同时清除选中（无持久聚焦）
// li 的 @click 冒泡到此，closest('li') 命中则保留选中
function onPanelClick(e: MouseEvent) {
  closeMenu()
  if (!(e.target as HTMLElement).closest('li')) selectedId.value = null
}

// 双击连接：连接后清除选中（聚焦是"离开"操作，不再保留高亮，用户反馈）
function onHostDblClick(host: Host) {
  selectedId.value = null
  emit('connect', host)
}

// 点击主机栏外部清除选中（与文件树失焦清除惯例一致）
useClearSelectionOnOutside(() => { selectedId.value = null }, '.host-panel')
</script>

<template>
  <div class="host-panel" @click="onPanelClick">
    <div class="sidebar-header">
      <h2>{{ t('hosts.title') }}</h2>
      <button class="btn btn-primary" @click="emit('new')">{{ t('hosts.add') }}</button>
    </div>
    <div class="search-bar">
      <input :value="searchQuery" type="text" :placeholder="t('hosts.searchPlaceholder')" @input="onSearchInput" />
    </div>
    <div class="host-list">
      <template v-for="([groupName, groupHosts], groupIdx) in groupedHosts" :key="(groupName || '__unassigned__') + ':' + groupIdx">
        <div class="group-header">{{ groupName || t('hosts.groupUnassigned') }}</div>
        <ul>
          <li v-for="host in groupHosts" :key="host.id"
            :class="{ active: selectedId === host.id }"
            @click="selectedId = host.id"
            @dblclick="onHostDblClick(host)"
            @mouseenter="showTip($event, host)"
            @mouseleave="hideTip"
            @contextmenu="onContextMenu($event, host)">
            <span class="host-main">
              <span class="host-name">{{ host.name }}</span>
              <!-- 收藏星标（SVG 无 emoji）：点击切换收藏，不触发行其他操作；
                   收藏金色高亮，未收藏弱显示（成熟客户端惯例） -->
              <svg class="star" :class="{ active: host.favorite }"
                :title="host.favorite ? t('hosts.unfavorite') : t('form.favorite')"
                viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"
                stroke-linecap="round" stroke-linejoin="round"
                @click.stop="emit('toggle-favorite', host)">
                <path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01z" />
              </svg>
            </span>
            <span class="host-addr">{{ host.address }}:{{ host.port }}</span>
          </li>
        </ul>
      </template>
    </div>
    <div v-if="menu" class="context-menu" :style="menuStyle">
      <div class="menu-item" @click="pick('connect')">{{ t('common.connect') }}</div>
      <div class="menu-item" @click="pick('edit')">{{ t('common.edit') }}</div>
      <div class="menu-item" @click="pick('ping')">{{ t('common.ping') }}</div>
      <div class="menu-item danger" @click="pick('delete')">{{ t('common.delete') }}</div>
    </div>
    <!-- 悬停信息卡：纯展示（pointer-events none 不拦截鼠标）；备注为空时省略该行 -->
    <div v-if="tip" class="host-tip" :style="{ left: tip.x + 'px', top: tip.y + 'px' }">
      <div class="tip-title">{{ tip.host.name }}</div>
      <div class="tip-addr">{{ tip.host.address }}:{{ tip.host.port }}</div>
      <div class="tip-grid">
        <span class="tip-label">{{ t('detail.username') }}</span><span>{{ tip.host.username }}</span>
        <span class="tip-label">{{ t('detail.auth') }}</span><span>{{ authLabel(tip.host.auth_type) }}</span>
        <span class="tip-label">{{ t('detail.group') }}</span><span>{{ tip.host.group_name || t('hosts.groupUnassigned') }}</span>
        <template v-if="tip.host.notes">
          <span class="tip-label">{{ t('detail.notes') }}</span><span class="tip-notes">{{ tip.host.notes }}</span>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 主机栏整体：右侧区域内的上半部分（flex:1 上下贯通）；宽度跟随 --sidebar-width CSS 变量（splitter 拖拽联动），未设置时回退默认 220px */
.host-panel { width: var(--sidebar-width, 220px); min-width: var(--sidebar-width, 220px); background: var(--color-sidebar); display: flex; flex-direction: column; flex: 1; min-height: 0; }
.sidebar-header { display: flex; justify-content: space-between; align-items: center; height: var(--bar-height); padding: 0 0.8rem; border-bottom: 1px solid var(--color-border); }
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
.host-main { display: flex; align-items: center; justify-content: space-between; gap: 0.4rem; min-width: 0; }
.host-name { font-weight: 600; color: var(--color-heading); font-size: 0.85rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.host-addr { font-size: 0.7rem; color: var(--color-text); opacity: 0.7; }
/* 收藏星标：未收藏弱显示（悬停变亮），收藏金色高亮 */
.star { width: 13px; height: 13px; flex-shrink: 0; color: var(--color-text); opacity: 0.25; cursor: pointer; transition: opacity 0.15s; }
.star:hover { opacity: 0.9; }
.star.active { opacity: 1; color: var(--color-warning); fill: var(--color-warning); }

/* context-menu 沿用 RemoteFileTree 样式；删除项红色警示 */
.context-menu {
  position: fixed; background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 4px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); z-index: 1000; min-width: 120px;
}
.menu-item { padding: 0.3rem 0.8rem; cursor: pointer; font-size: 0.8rem; }
.menu-item:hover { background: var(--color-background-mute); }
.menu-item.danger { color: var(--color-danger); }

/* 悬停信息卡：与 context-menu 同风格浮层；pointer-events none 保证不干扰鼠标悬停切换 */
.host-tip {
  position: fixed; width: 200px; padding: 0.6rem 0.7rem;
  background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 4px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); z-index: 1000;
  pointer-events: none; animation: tip-in 0.12s ease-out;
}
.tip-title { font-size: 0.85rem; font-weight: 600; color: var(--color-heading); margin-bottom: 0.15rem; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.tip-addr { font-size: 0.72rem; color: var(--color-text); opacity: 0.7; font-family: Consolas, monospace; margin-bottom: 0.5rem; }
.tip-grid { display: grid; grid-template-columns: auto 1fr; gap: 0.25rem 0.8rem; font-size: 0.75rem; }
.tip-label { color: var(--color-text); opacity: 0.55; white-space: nowrap; }
.tip-notes { white-space: pre-wrap; word-break: break-all; }
@keyframes tip-in { from { opacity: 0; } to { opacity: 1; } }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn-primary { background: var(--color-accent); color: #fff; border-color: var(--color-accent); }
.btn-primary:hover { background: color-mix(in srgb, var(--color-accent), black 12%); }
</style>
