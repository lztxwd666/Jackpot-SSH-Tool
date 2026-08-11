<script setup lang="ts">
import { ref, reactive, computed, onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
import { formatFileSize, isValidNewName, resolveNameConflict, copyPath, parseDragPayload, type DragItem } from '../composables/fs'
import { clampFloatPos } from '../composables/pos'
import { useClickOutsideClose } from '../composables/menu'
import { useClearSelectionOnOutside } from '../composables/selection'
import FileTreeHeader from './FileTreeHeader.vue'
import FileIcon from './FileIcon.vue'
import { t } from '../composables/i18n'

// 自定义 MIME 类型：dragover 期间 getData() 不可读，只能读 types
// 因此用自定义类型区分本地/远程拖拽，drop 时再取实际路径
const LOCAL_DRAG_TYPE = 'application/x-jackpot-local'
const REMOTE_DRAG_TYPE = 'application/x-jackpot-remote'

const props = defineProps<{ refreshKey: number }>()

const emit = defineEmits<{
  (e: 'download', remotePath: string, localDir: string, isDir?: boolean): void
  (e: 'download-many', items: DragItem[], localDir: string): void
  (e: 'upload-many', items: DragItem[]): void
  (e: 'current-dir', path: string): void
  (e: 'upload-request', localPath: string, isDir?: boolean): void
}>()

interface FileNode {
  name: string
  path: string
  is_dir: boolean
  size: number
}

const currentPath = ref('C:\\')
const files = ref<FileNode[]>([])
// 多选选区（VSCode 交互：Ctrl 点选切换 / Shift 范围选 / 普通单击单选）
// reactive 支持 Set 的响应式；anchor 为 Shift 范围选的锚点（上次点击项）
const selected = reactive(new Set<string>())
let anchor: string | null = null
const loading = ref(false)
const dragOver = ref(false)  // 拖拽悬停高亮

// 右键菜单状态（文件/目录项）
const showMenu = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuTarget = ref<FileNode | null>(null)
// 菜单位置 clamp（估算宽高，防贴窗口右缘/下缘时溢出被裁剪）
const menuStyle = computed(() => {
  const p = clampFloatPos(menuX.value, menuY.value, 140, 140)
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
// 失焦清除选区：点击本地树外部（含另一棵树）取消选中（VSCode 行为）
useClearSelectionOnOutside(() => {
  selected.clear()
  anchor = null
}, '.file-tree--local')

// 点击文件树内部：空白区域（未命中节点与标题栏）同样清除选区；
// 标题栏按钮点击不清除（新建操作不应丢失原选区）
function onTreeClick(e: MouseEvent) {
  closeMenu()
  if (!(e.target as HTMLElement).closest('.tree-node, .tree-header')) {
    selected.clear()
    anchor = null
  }
}

function parentPath(p: string): string {
  const stripped = p.replace(/\\$/, '')
  if (stripped.length === 2 && stripped.endsWith(':')) return ''
  const idx = Math.max(stripped.lastIndexOf('\\'), stripped.lastIndexOf('/'))
  if (idx < 0) return ''
  const parent = stripped.substring(0, idx)
  if (parent.length === 2 && parent.endsWith(':')) return parent + '\\'
  return parent
}

// 请求序号：快速连续切换目录时旧响应不得覆盖新目录（响应乱序导致
// 列表与 currentPath 错配，右键删除/重命名会作用到错误目录）
let loadSeq = 0
async function loadDir(path: string) {
  const seq = ++loadSeq
  loading.value = true
  try {
    const result = await invoke('read_local_dir', { path }) as FileNode[]
    if (seq !== loadSeq) return // 过期响应：已有更新的请求
    files.value = result
    currentPath.value = path
    // 目录切换：旧目录的选区失效
    selected.clear()
    anchor = null
    emit('current-dir', path)
  } catch (e) {
    if (seq !== loadSeq) return
    console.error('read_local_dir failed:', e)
  }
  loading.value = false
}

// ---- 拖拽：本地文件/目录拖出（上传到远程；目录走递归传输） ----
function onDragStart(e: DragEvent, file: FileNode) {
  const dt = e.dataTransfer!
  // 拖拽项在多选选区中：拖整个选区；否则拖单项（VSCode 交互）
  let items: { path: string; isDir: boolean }[]
  if (selected.has(file.path) && selected.size > 1) {
    items = files.value.filter(f => selected.has(f.path)).map(f => ({ path: f.path, isDir: f.is_dir }))
  } else {
    items = [{ path: file.path, isDir: file.is_dir }]
  }
  dt.setData(LOCAL_DRAG_TYPE, JSON.stringify({ items }))
  dt.effectAllowed = 'copy'
}

// 远程文件拖入（下载到本地）：无条件接受悬停，drop 时校验数据
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
  const raw = dt.getData(REMOTE_DRAG_TYPE)
  if (!raw) return
  // 解析载荷（多选数组 / 单选对象 / 旧格式）；批量一次投递，由 App.vue 串行执行
  const items = parseDragPayload(raw)
  if (items.length === 1) {
    emit('download', items[0].path, currentPath.value, items[0].isDir)
  } else {
    emit('download-many', items, currentPath.value)
  }
}

// 单击交互（VSCode 对齐）：Ctrl 点选切换 / Shift 范围选 / 普通单击仅选中
// （文件夹不导航，进入目录由双击承担；多选文件夹自然可用）
function onClickItem(e: MouseEvent, file: FileNode) {
  if (e.ctrlKey || e.metaKey) {
    if (selected.has(file.path)) selected.delete(file.path)
    else selected.add(file.path)
    anchor = file.path
    return
  }
  if (e.shiftKey) {
    rangeSelect(file.path)
    return
  }
  selected.clear()
  selected.add(file.path)
  anchor = file.path
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
function enterDir(file: FileNode) {
  if (!file.is_dir) return
  loadDir(file.path)
}

function goUp() {
  loadDir(parentPath(currentPath.value))
}

// ---- 右键菜单操作 ----
function onContextMenu(e: MouseEvent, file: FileNode) {
  e.preventDefault()
  // 右键项不在选区中：重置选区为该单项（VSCode 交互）；在选区中则保持（批量操作入口）
  if (!selected.has(file.path)) {
    selected.clear()
    selected.add(file.path)
    anchor = file.path
  }
  menuTarget.value = file
  menuX.value = e.clientX
  menuY.value = e.clientY
  showMenu.value = true
}

// 批量上传（多选）：一次性投递批量事件，由 App.vue 串行执行
// （逐项循环 emit 会并发发起，worker 单线程 transferring 互斥导致其余报 busy）
function doUploadMany() {
  if (!menuTarget.value) return
  closeMenu()
  const items = files.value.filter(f => selected.has(f.path)).map(f => ({ path: f.path, isDir: f.is_dir }))
  emit('upload-many', items)
}

// 批量删除（多选）：确认后逐项删除（彻底删除，无回收站，必须确认）
async function doDeleteMany() {
  const targets = files.value.filter(f => selected.has(f.path))
  if (!targets.length) return
  closeMenu()
  const ok = await confirmDialog(t('tree.deleteManyConfirm', { n: String(targets.length) }))
  if (!ok) return
  let failed = 0
  for (const item of targets) {
    try {
      await invoke('delete_local_file', { path: item.path, isDir: item.is_dir })
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

// 上传到远程（文件或目录，目录走递归传输）
function doUploadToRemote() {
  if (!menuTarget.value) return
  closeMenu()
  emit('upload-request', menuTarget.value.path, menuTarget.value.is_dir)
}

// 新建文件夹（dir 缺省为当前目录；右键文件夹场景传该文件夹路径，在该文件夹下创建）
async function doNewFolder(dir?: string) {
  closeMenu()
  closeBlankMenu()
  const name = await promptDialog(t('prompt.folderName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）；冲突判定基于目标目录的现有条目
  const base = dir ?? currentPath.value
  const existing = dir ? await listNamesIn(base) : new Set(files.value.map(f => f.name))
  const final = await resolveNameConflict(name, existing)
  if (!final) return
  const path = base.replace(/\\$/, '') + '\\' + final
  try {
    await invoke('create_local_dir', { path })
    loadDir(base)
    showToast(t('toast.created', { name: final }), 'success')
  } catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

// 新建文件（dir 缺省为当前目录；右键文件夹场景传该文件夹路径）
async function doNewFile(dir?: string) {
  closeMenu()
  closeBlankMenu()
  const name = await promptDialog(t('prompt.fileName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）
  const base = dir ?? currentPath.value
  const existing = dir ? await listNamesIn(base) : new Set(files.value.map(f => f.name))
  const final = await resolveNameConflict(name, existing)
  if (!final) return
  const path = base.replace(/\\$/, '') + '\\' + final
  try {
    // 空内容创建（write_local_file 自动建父目录，此处父目录即创建目录）
    await invoke('write_local_file', { path, data: [] })
    loadDir(base)
    showToast(t('toast.created', { name: final }), 'success')
  } catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

// 列出指定目录的名字（用于子文件夹内新建的冲突判定；失败时按无冲突处理）
async function listNamesIn(dir: string): Promise<Set<string>> {
  try {
    const entries = await invoke('read_local_dir', { path: dir }) as FileNode[]
    return new Set(entries.map(f => f.name))
  } catch {
    return new Set()
  }
}

async function doRename() {
  if (!menuTarget.value) return
  closeMenu()
  const target = menuTarget.value
  const newName = await promptDialog(t('prompt.newName'), target.name)
  if (!newName || newName === target.name) return
  // 校验名称：拒绝路径分隔符与 ..（防移动到目录外）
  if (!isValidNewName(newName)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突（排除自身）：重命名不给"覆盖"选项（Windows rename 目标存在即失败）
  const others = new Set(files.value.filter(f => f.name !== target.name).map(f => f.name))
  const final = await resolveNameConflict(newName, others, { allowOverwrite: false })
  if (!final) return
  const parent = currentPath.value.replace(/\\$/, '')
  try {
    await invoke('rename_local_file', { oldPath: target.path, newPath: parent + '\\' + final })
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

async function doDelete() {
  if (!menuTarget.value) return
  closeMenu()
  const target = menuTarget.value
  const ok = await confirmDialog(t('hosts.deleteConfirm', { name: target.name }))
  if (!ok) return
  try {
    await invoke('delete_local_file', { path: target.path, isDir: target.is_dir })
    loadDir(currentPath.value)
    showToast(t('toast.deleted', { name: target.name }), 'success')
  } catch (e) { showToast(t('toast.deleteFailed', { err: String(e) }), 'error', 5000) }
}

function refresh() {
  closeBlankMenu()
  loadDir(currentPath.value)
}

onMounted(() => loadDir(''))

// 外部刷新令牌（下载完成后自动刷新当前目录）
watch(() => props.refreshKey, () => { loadDir(currentPath.value) })
</script>

<template>
  <div class="file-tree file-tree--local" :class="{ 'drag-over': dragOver }" @drop.prevent="onDrop" @dragenter="onDragenter"
    @dragover="onDragover" @dragleave="onDragLeave" @click="onTreeClick">
    <!-- VSCode EXPLORER 样式标题栏：标题居左，悬停显示新建文件/文件夹/刷新 -->
    <FileTreeHeader :title="t('tree.localTitle')" @new-file="doNewFile" @new-folder="doNewFolder" @refresh="refresh" />
    <div class="tree-body" @contextmenu="onBlankContextMenu">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="currentPath && parentPath(currentPath) !== currentPath" class="tree-node up-node" @click="goUp">
        <!-- 父目录图标：.. 不匹配任何目录名，回落默认文件夹图标 -->
        <FileIcon name=".." is-dir class="icon" />
        <span class="name">..</span>
      </div>
      <div v-for="file in files" :key="file.path" class="tree-node" :class="{ selected: selected.has(file.path) }"
        :draggable="true" @click="onClickItem($event, file)" @dblclick="enterDir(file)"
        @dragstart="onDragStart($event, file)" @contextmenu="onContextMenu($event, file)">
        <!-- 文件/文件夹类型图标：按扩展名/目录名解析（material 图标主题，来源见 assets/icons/material/LICENSE） -->
        <FileIcon :name="file.name" :is-dir="file.is_dir" class="icon" />
        <span class="name">{{ file.name }}</span>
        <span v-if="!file.is_dir" class="size">{{ formatFileSize(file.size) }}</span>
      </div>
    </div>

    <!-- 右键菜单：多选为批量操作，单选为原有单项操作（VSCode 交互） -->
    <div v-if="showMenu" class="context-menu" :style="menuStyle">
      <template v-if="menuTarget && selected.size > 1">
        <div class="menu-item" @click="doUploadMany">{{ t('tree.uploadN', { n: String(selected.size) }) }}</div>
        <div class="menu-item" @click="doDeleteMany">{{ t('tree.deleteN', { n: String(selected.size) }) }}</div>
      </template>
      <template v-else>
        <!-- 上传（文件/目录均可，目录走递归传输） -->
        <div v-if="menuTarget" class="menu-item" @click="doUploadToRemote">{{ t('common.upload') }}</div>
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
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--color-panel);
  user-select: none;
  font-size: 0.8rem;
}

.tree-header {
  padding: 0.4rem 0.5rem;
  font-weight: 600;
  color: var(--color-heading);
  border-bottom: 1px solid var(--color-border);
  text-align: center;
  flex-shrink: 0;
}

.tree-body {
  flex: 1;
  overflow-y: auto;
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 0.3rem;
  padding: 0.2rem 0.5rem;
  cursor: pointer;
  white-space: nowrap;
}

.tree-node:hover {
  background: var(--color-panel-mute);
}

.tree-node.selected {
  background: var(--color-panel-selected);
}

.up-node {
  border-bottom: 1px solid var(--color-border);
  font-weight: 500;
}

.icon {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
}

.size {
  font-size: 0.7rem;
  opacity: 0.5;
  flex-shrink: 0;
  white-space: nowrap;
}

.loading {
  padding: 0.5rem;
  opacity: 0.5;
  text-align: center;
}

.drag-over {
  outline: 2px solid var(--color-accent);
  outline-offset: -2px;
  background: rgba(46, 160, 67, 0.08);
}

.context-menu {
  position: fixed;
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  z-index: 1000;
  min-width: 120px;
}

.menu-item {
  padding: 0.3rem 0.8rem;
  cursor: pointer;
  font-size: 0.8rem;
}

.menu-item:hover {
  background: var(--color-background-mute);
}
</style>
