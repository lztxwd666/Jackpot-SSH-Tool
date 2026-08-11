<script setup lang="ts">
import { ref, reactive, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
import { formatFileSize, isValidNewName, resolveNameConflict, copyPath, parseDragPayload, type DragItem } from '../composables/fs'
import { clampFloatPos } from '../composables/pos'
import { useClickOutsideClose } from '../composables/menu'
import { useClearSelectionOnOutside } from '../composables/selection'
import FileTreeHeader from './FileTreeHeader.vue'
import { t } from '../composables/i18n'

// 自定义 MIME 类型：dragover 期间 getData() 不可读，只能读 types
const LOCAL_DRAG_TYPE = 'application/x-jackpot-local'
const REMOTE_DRAG_TYPE = 'application/x-jackpot-remote'

const props = defineProps<{ sessionId: string; refreshKey: number; locked?: boolean }>()

const emit = defineEmits<{
  (e: 'download', remotePath: string, isDir?: boolean): void
  (e: 'download-many', items: DragItem[]): void
  (e: 'upload', remoteDir: string, localPath: string, isDir?: boolean): void
  (e: 'upload-many', items: DragItem[], remoteDir: string): void
  (e: 'current-dir', path: string): void
}>()

interface FileNode {
  name: string
  path: string
  size: number
  is_dir: boolean
  modified: string
}

const currentPath = ref('/')
const files = ref<FileNode[]>([])
// 多选选区（VSCode 交互：Ctrl 点选切换 / Shift 范围选 / 普通单击单选）
const selected = reactive(new Set<string>())
let anchor: string | null = null
const loading = ref(false)
const error = ref('')
const showMenu = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuTarget = ref<FileNode | null>(null)
// 菜单位置 clamp（估算宽高，防贴窗口右缘/下缘时溢出被裁剪）
const menuStyle = computed(() => {
  const p = clampFloatPos(menuX.value, menuY.value, 140, 150)
  return { left: p.x + 'px', top: p.y + 'px' }
})

// 空白区域右键菜单（新建文件/新建文件夹/刷新，VSCode 资源管理器对齐）
const blankMenu = ref<{ x: number; y: number } | null>(null)
const blankMenuStyle = computed(() => {
  if (!blankMenu.value) return { left: '0px', top: '0px' }
  const p = clampFloatPos(blankMenu.value.x, blankMenu.value.y, 140, 110)
  return { left: p.x + 'px', top: p.y + 'px' }
})
function onBlankContextMenu(e: MouseEvent) {
  // 空白区域与".."父目录节点显示新建菜单（up-node 非可操作文件项）
  if ((e.target as HTMLElement).closest('.tree-node:not(.up-node)')) return
  e.preventDefault()
  blankMenu.value = { x: e.clientX, y: e.clientY }
}
function closeBlankMenu() { blankMenu.value = null }

// 菜单打开时注册全局点击关闭（点击菜单外任意处消除菜单，标准交互）
useClickOutsideClose(showMenu, closeMenu)
useClickOutsideClose(blankMenu, closeBlankMenu)
// 失焦清除选区：点击远程树外部（含另一棵树）取消选中（VSCode 行为）
useClearSelectionOnOutside(() => {
  selected.clear()
  anchor = null
}, '.file-tree--remote')

// 点击文件树内部：空白区域（未命中节点与标题栏）同样清除选区；
// 标题栏按钮点击不清除（新建操作不应丢失原选区）
function onTreeClick(e: MouseEvent) {
  closeMenu()
  if (!(e.target as HTMLElement).closest('.tree-node, .tree-header')) {
    selected.clear()
    anchor = null
  }
}
const loaded = ref(false)
const dragOver = ref(false)  // 拖拽悬停高亮

function parentPath(p: string): string {
  if (p === '/') return '/'
  const parts = p.replace(/\/$/, '').split('/').filter(Boolean)
  parts.pop()
  return '/' + parts.join('/')
}

// 请求序号：快速连续切换目录时旧响应不得覆盖新目录（响应乱序导致
// 列表与 currentPath 错配，右键删除/重命名会作用到错误目录）
let loadSeq = 0
async function loadDir(path: string) {
  if (!props.sessionId) return
  const seq = ++loadSeq
  loading.value = true; error.value = ''
  try {
    const result = await invoke('sftp_list_dir', { sessionId: props.sessionId, path }) as FileNode[]
    if (seq !== loadSeq) return // 过期响应：已有更新的请求
    files.value = result
    currentPath.value = path
    // 目录切换：旧目录的选区失效
    selected.clear()
    anchor = null
    loaded.value = true
    emit('current-dir', path)
  } catch (e) {
    if (seq !== loadSeq) return
    error.value = String(e)
    console.error('sftp_list_dir failed:', e)
  }
  loading.value = false
}

// 单击交互（VSCode 对齐）：Ctrl 点选切换 / Shift 范围选 / 普通单击仅选中
// （文件夹不导航，进入目录由双击承担；多选文件夹自然可用）
function onClickItem(e: MouseEvent, f: FileNode) {
  if (e.ctrlKey || e.metaKey) {
    if (selected.has(f.path)) selected.delete(f.path)
    else selected.add(f.path)
    anchor = f.path
    return
  }
  if (e.shiftKey) {
    rangeSelect(f.path)
    return
  }
  selected.clear()
  selected.add(f.path)
  anchor = f.path
}

// Shift 范围选：从锚点到当前项之间的全部项（列表顺序）
// 首次 Shift 点击（anchor 为空）把当前项设为锚点（VSCode 行为：先按 Shift 再
// 接连点击同样生效）；锚点在后续 Shift 点击中保持（范围以最初锚点扩展/收缩）
function rangeSelect(path: string) {
  const paths = files.value.map(f => f.path)
  const cur = paths.indexOf(path)
  if (cur < 0) {
    selected.clear()
    anchor = null
    return
  }
  if (!anchor) anchor = path
  const anc = paths.indexOf(anchor)
  if (anc < 0) {
    selected.clear()
    selected.add(path)
    anchor = path
    return
  }
  const [start, end] = cur < anc ? [cur, anc] : [anc, cur]
  selected.clear()
  for (let i = start; i <= end; i++) selected.add(paths[i])
}

// 双击进入目录（VSCode 对齐：单击选中，双击进入）
function enterDir(f: FileNode) {
  if (!f.is_dir) return
  loadDir(f.path)
}

function goUp() {
  loadDir(parentPath(currentPath.value))
}

function refresh() {
  closeBlankMenu()
  loadDir(currentPath.value)
}

// ---- 拖拽：远程文件/目录拖出（下载到本地；目录走递归传输） ----
function onDragStart(e: DragEvent, f: FileNode) {
  const dt = e.dataTransfer!
  // 拖拽项在多选选区中：拖整个选区；否则拖单项（VSCode 交互）
  let items: { path: string; isDir: boolean }[]
  if (selected.has(f.path) && selected.size > 1) {
    items = files.value.filter(x => selected.has(x.path)).map(x => ({ path: x.path, isDir: x.is_dir }))
  } else {
    items = [{ path: f.path, isDir: f.is_dir }]
  }
  dt.setData(REMOTE_DRAG_TYPE, JSON.stringify({ items }))
  dt.effectAllowed = 'copy'
}

// 本地文件拖入（上传）：无条件接受悬停，drop 时校验数据
// dragenter 与 dragover 都必须 preventDefault 才允许放置：只处理 dragover 时，
// 鼠标在子元素间移动会反复触发 dragenter，光标在"允许/禁止"间闪烁
function onDragenter(e: DragEvent) {
  e.preventDefault()
  e.dataTransfer!.dropEffect = 'copy'
}
function onDragover(e: DragEvent) {
  e.preventDefault()
  e.dataTransfer!.dropEffect = 'copy'
  dragOver.value = true
}

function onDragLeave() {
  dragOver.value = false
}

function onDrop(e: DragEvent) {
  dragOver.value = false
  const dt = e.dataTransfer
  if (!dt) return
  // 从系统文件管理器拖入的文件拿不到真实路径，提示改用本地文件树
  if (dt.files && dt.files.length > 0) {
    showToast(t('toast.systemFileDrop'), 'warning', 6000)
    return
  }
  // 仅信任本应用内拖拽（自定义 MIME）：外部 text/plain 载荷不可信（可被伪造注入路径）
  const raw = dt.getData(LOCAL_DRAG_TYPE)
  if (!raw) return
  // 解析载荷（多选数组 / 单选对象 / 旧格式）；批量一次投递，由 App.vue 串行执行
  const items = parseDragPayload(raw)
  if (items.length === 1) {
    emit('upload', currentPath.value, items[0].path, items[0].isDir)
  } else {
    emit('upload-many', items, currentPath.value)
  }
}

// 右键菜单
function onContextMenu(e: MouseEvent, f: FileNode) {
  e.preventDefault()
  // 右键项不在选区中：重置选区为该单项（VSCode 交互）；在选区中则保持（批量操作入口）
  if (!selected.has(f.path)) {
    selected.clear()
    selected.add(f.path)
    anchor = f.path
  }
  menuTarget.value = f
  menuX.value = e.clientX
  menuY.value = e.clientY
  showMenu.value = true
}

// 批量下载（多选）：一次性投递批量事件，由 App.vue 串行执行
// （逐项循环 emit 会并发发起，worker 单线程 transferring 互斥导致其余报 busy）
function doDownloadMany() {
  if (!menuTarget.value) return
  closeMenu()
  const items = files.value.filter(f => selected.has(f.path)).map(f => ({ path: f.path, isDir: f.is_dir }))
  emit('download-many', items)
}

// 批量删除（多选）：确认后逐项删除（远端无回收站，必须确认）
async function doDeleteMany() {
  const targets = files.value.filter(f => selected.has(f.path))
  if (!targets.length || !props.sessionId) return
  closeMenu()
  const ok = await confirmDialog(t('tree.deleteManyConfirm', { n: String(targets.length) }))
  if (!ok) return
  let failed = 0
  for (const item of targets) {
    try {
      await invoke('sftp_delete', { sessionId: props.sessionId, path: item.path, isDir: item.is_dir })
    } catch (e) {
      failed++
      showToast(t('toast.deleteFailed', { err: String(e) }), 'error', 5000)
    }
  }
  loadDir(currentPath.value)
  if (failed === 0) {
    showToast(t('tree.deletedN', { n: String(targets.length) }), 'success')
  }
}
function closeMenu() { showMenu.value = false }

async function doDelete() {
  if (!menuTarget.value || !props.sessionId) return
  closeMenu()
  const target = menuTarget.value
  const ok = await confirmDialog(t('hosts.deleteConfirm', { name: target.name }))
  if (!ok) return
  try {
    await invoke('sftp_delete', { sessionId: props.sessionId, path: target.path, isDir: target.is_dir })
    loadDir(currentPath.value)
    showToast(t('toast.deleted', { name: target.name }), 'success')
  } catch (e) { showToast(t('toast.deleteFailed', { err: String(e) }), 'error', 5000) }
}
async function doRename() {
  if (!menuTarget.value || !props.sessionId) return
  closeMenu()
  const target = menuTarget.value
  const newName = await promptDialog(t('prompt.newName'), target.name)
  if (!newName || newName === target.name) return
  // 校验名称：拒绝路径分隔符与 ..（防移动到目录外）
  if (!isValidNewName(newName)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突（排除自身）：重命名不给"覆盖"选项（SFTP rename 目标存在即失败）
  const others = new Set(files.value.filter(f => f.name !== target.name).map(f => f.name))
  const final = await resolveNameConflict(newName, others, { allowOverwrite: false })
  if (!final) return
  const parent = currentPath.value.replace(/\/$/, '')
  try {
    await invoke('sftp_rename', { sessionId: props.sessionId, oldPath: target.path, newPath: parent + '/' + final })
    loadDir(currentPath.value)
    showToast(t('toast.renamed', { name: final }), 'success')
  } catch (e) { showToast(t('toast.renameFailed', { err: String(e) }), 'error', 5000) }
}

// 复制完整路径到剪贴板（运维常用，VSCode 同款）
async function doCopyPath() {
  if (!menuTarget.value) return
  closeMenu()
  const ok = await copyPath(menuTarget.value.path)
  showToast(ok ? t('toast.copied') : t('toast.copyFailed'), ok ? 'success' : 'error')
}
// 新建文件夹（dir 缺省为当前目录；右键文件夹场景传该文件夹路径，在该文件夹下创建）
async function doNewFolder(dir?: string) {
  if (!props.sessionId) return; closeMenu(); closeBlankMenu()
  const name = await promptDialog(t('prompt.folderName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）；冲突判定基于目标目录的现有条目
  const base = dir ?? currentPath.value
  const existing = dir ? await listNamesIn(base) : new Set(files.value.map(f => f.name))
  const final = await resolveNameConflict(name, existing)
  if (!final) return
  const path = base.replace(/\/$/, '') + '/' + final
  try {
    await invoke('sftp_create_dir', { sessionId: props.sessionId, path })
    loadDir(base)
    showToast(t('toast.created', { name: final }), 'success')
  }
  catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

// 新建文件（dir 缺省为当前目录；右键文件夹场景传该文件夹路径）
async function doNewFile(dir?: string) {
  if (!props.sessionId) return; closeMenu(); closeBlankMenu()
  const name = await promptDialog(t('prompt.fileName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）
  const base = dir ?? currentPath.value
  const existing = dir ? await listNamesIn(base) : new Set(files.value.map(f => f.name))
  const final = await resolveNameConflict(name, existing)
  if (!final) return
  const path = base.replace(/\/$/, '') + '/' + final
  try {
    await invoke('sftp_create_file', { sessionId: props.sessionId, path })
    loadDir(base)
    showToast(t('toast.created', { name: final }), 'success')
  }
  catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

// 列出指定目录的名字（用于子文件夹内新建的冲突判定；失败时按无冲突处理）
async function listNamesIn(dir: string): Promise<Set<string>> {
  if (!props.sessionId) return new Set()
  try {
    const entries = await invoke('sftp_list_dir', { sessionId: props.sessionId, path: dir }) as FileNode[]
    return new Set(entries.map(f => f.name))
  } catch {
    return new Set()
  }
}
// 下载（文件或目录，目录走递归传输）
function doDownload() {
  if (!menuTarget.value) return
  closeMenu(); emit('download', menuTarget.value.path, menuTarget.value.is_dir)
}

// sessionId 变化或刷新令牌变化时重新加载
watch(() => props.sessionId, (sid) => { if (sid) loadDir('/') }, { immediate: true })
watch(() => props.refreshKey, () => { if (props.sessionId) loadDir(currentPath.value) })
// 解锁时自动刷新目录（传输结束后文件列表可能已变化）
watch(() => props.locked, (locked) => {
  if (!locked && props.sessionId) loadDir(currentPath.value)
})
</script>

<template>
  <div class="file-tree file-tree--remote" :class="{ 'drag-over': dragOver, 'locked': locked }" @drop.prevent="onDrop" @dragenter="onDragenter" @dragover="onDragover" @dragleave="onDragLeave" @click="onTreeClick">
    <!-- VSCode EXPLORER 样式标题栏：标题居左，悬停显示新建文件/文件夹/刷新 -->
    <FileTreeHeader :title="t('tree.remoteTitle')" @new-file="doNewFile" @new-folder="doNewFolder" @refresh="refresh" />
    <!-- 锁定提示：锁图标 + 脉冲动画（与 SessionTab 状态条风格统一，SVG 无 emoji） -->
    <div v-if="locked" class="lock-banner">
      <svg class="lock-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />
      </svg>
      <span>{{ t('tree.transferLocked') }}</span>
    </div>
    <div class="tree-body" @contextmenu="onBlankContextMenu">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="error" class="error">{{ error }}</div>
      <div v-if="currentPath !== '/'" class="tree-node up-node" @click="goUp">
        <!-- 文件夹图标（琥珀色，SVG 无 emoji）：父目录 -->
        <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="#d29922" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
        <span class="name">..</span>
      </div>
      <div
        v-for="f in files"
        :key="f.path"
        class="tree-node"
        :class="{ selected: selected.has(f.path) }"
        :draggable="true"
        @click="onClickItem($event, f)"
        @dblclick="enterDir(f)"
        @dragstart="onDragStart($event, f)"
        @contextmenu="onContextMenu($event, f)"
      >
        <!-- 文件夹/文件图标（SVG 无 emoji，项目约定）：文件夹琥珀色、文件中性灰 -->
        <svg v-if="f.is_dir" class="icon" viewBox="0 0 24 24" fill="none" stroke="#d29922" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
        <svg v-else class="icon" viewBox="0 0 24 24" fill="none" stroke="#8b949e" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>
        <span class="name">{{ f.name }}</span>
        <span v-if="!f.is_dir" class="size">{{ formatFileSize(f.size) }}</span>
      </div>
    </div>

    <!-- 右键菜单：多选为批量操作，单选为原有单项操作（VSCode 交互） -->
    <div v-if="showMenu" class="context-menu" :style="menuStyle">
      <template v-if="menuTarget && selected.size > 1">
        <div class="menu-item" @click="doDownloadMany">{{ t('tree.downloadN', { n: String(selected.size) }) }}</div>
        <div class="menu-item" @click="doDeleteMany">{{ t('tree.deleteN', { n: String(selected.size) }) }}</div>
      </template>
      <template v-else>
        <!-- 下载（文件/目录均可，目录走递归传输） -->
        <div v-if="menuTarget" class="menu-item" @click="doDownload">{{ t('common.download') }}</div>
        <template v-if="menuTarget && menuTarget.is_dir">
          <div class="menu-item" @click="doNewFile(menuTarget.path)">{{ t('common.newFile') }}</div>
          <div class="menu-item" @click="doNewFolder(menuTarget.path)">{{ t('common.newFolder') }}</div>
        </template>
        <div v-if="menuTarget" class="menu-item" @click="doRename">{{ t('common.rename') }}</div>
        <div v-if="menuTarget" class="menu-item" @click="doCopyPath">{{ t('common.copyPath') }}</div>
        <div v-if="menuTarget" class="menu-item" @click="doDelete">{{ t('common.delete') }}</div>
      </template>
    </div>
    <!-- 空白区域右键菜单：新建文件/新建文件夹/刷新（VSCode 资源管理器对齐） -->
    <div v-if="blankMenu" class="context-menu" :style="blankMenuStyle" @click="closeBlankMenu">
      <div class="menu-item" @click="doNewFile()">{{ t('common.newFile') }}</div>
      <div class="menu-item" @click="doNewFolder()">{{ t('common.newFolder') }}</div>
      <div class="menu-item" @click="refresh">{{ t('common.refresh') }}</div>
    </div>
  </div>
</template>

<style scoped>
.file-tree {
  display: flex; flex-direction: column; height: 100%;
  background: var(--color-background-soft); border-left: 1px solid var(--color-border);
  user-select: none; font-size: 0.8rem; position: relative;
}
.tree-body { flex: 1; overflow-y: auto; }
.tree-node {
  display: flex; align-items: center; gap: 0.3rem; padding: 0.2rem 0.5rem;
  cursor: pointer; white-space: nowrap;
}
.tree-node:hover { background: var(--color-background-mute); }
.tree-node.selected { background: var(--color-border-hover); }
.up-node { border-bottom: 1px solid var(--color-border); font-weight: 500; }
.icon { flex-shrink: 0; width: 14px; height: 14px; }
.name { overflow: hidden; text-overflow: ellipsis; flex: 1; }
.size { font-size: 0.7rem; opacity: 0.5; flex-shrink: 0; white-space: nowrap; }
.loading { padding: 0.5rem; opacity: 0.5; text-align: center; }
.error { padding: 0.5rem; color: #e5534b; font-size: 0.75rem; text-align: center; }
.locked { pointer-events: none; opacity: 0.6; }
.lock-banner {
  display: flex; align-items: center; justify-content: center; gap: 0.35rem;
  padding: 0.35rem 0.5rem; font-size: 0.72rem; text-align: center;
  background: rgba(210, 153, 34, 0.15); color: #d29922;
  animation: banner-in 0.18s ease-out;
}
.lock-icon { width: 11px; height: 11px; flex-shrink: 0; animation: icon-pulse 1.6s ease-in-out infinite; }
@keyframes banner-in { from { opacity: 0; transform: translateY(-2px); } to { opacity: 1; transform: none; } }
@keyframes icon-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }
.drag-over {
  outline: 2px solid hsla(160, 100%, 37%, 1);
  outline-offset: -2px;
  background: rgba(46, 160, 67, 0.08);
}
.context-menu {
  position: fixed; background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 4px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); z-index: 1000; min-width: 120px;
}
.menu-item { padding: 0.3rem 0.8rem; cursor: pointer; font-size: 0.8rem; }
.menu-item:hover { background: var(--color-background-mute); }
</style>
