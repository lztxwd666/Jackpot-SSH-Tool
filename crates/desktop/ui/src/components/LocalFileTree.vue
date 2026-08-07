<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
import { formatFileSize, isValidNewName, resolveNameConflict, copyPath } from '../composables/fs'
import { clampFloatPos } from '../composables/pos'
import FileTreeHeader from './FileTreeHeader.vue'
import { t } from '../composables/i18n'

// 自定义 MIME 类型：dragover 期间 getData() 不可读，只能读 types
// 因此用自定义类型区分本地/远程拖拽，drop 时再取实际路径
const LOCAL_DRAG_TYPE = 'application/x-jackpot-local'
const REMOTE_DRAG_TYPE = 'application/x-jackpot-remote'

const props = defineProps<{ refreshKey: number }>()

const emit = defineEmits<{
  (e: 'download', remotePath: string, localDir: string): void
  (e: 'current-dir', path: string): void
  (e: 'upload-request', localPath: string): void
}>()

interface FileNode {
  name: string
  path: string
  is_dir: boolean
  size: number
}

const currentPath = ref('C:\\')
const files = ref<FileNode[]>([])
const selected = ref('')
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
  // 仅空白区域（未命中文件节点）显示新建菜单
  if ((e.target as HTMLElement).closest('.tree-node')) return
  e.preventDefault()
  blankMenu.value = { x: e.clientX, y: e.clientY }
}
function closeBlankMenu() { blankMenu.value = null }

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
    emit('current-dir', path)
  } catch (e) {
    if (seq !== loadSeq) return
    console.error('read_local_dir failed:', e)
  }
  loading.value = false
}

// ---- 拖拽：本地文件拖出（上传到远程） ----
function onDragStart(e: DragEvent, file: FileNode) {
  if (file.is_dir) return
  const dt = e.dataTransfer!
  dt.setData(LOCAL_DRAG_TYPE, file.path)
  dt.setData('text/plain', 'local:' + file.path)
  dt.effectAllowed = 'copy'
}

// 远程文件拖入（下载到本地）：无条件接受悬停，drop 时校验数据
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
  const remotePath = dt.getData(REMOTE_DRAG_TYPE)
  if (remotePath) {
    emit('download', remotePath, currentPath.value)
  }
}

function enterDir(file: FileNode) {
  if (file.is_dir) loadDir(file.path)
  selected.value = file.path
}

function goUp() {
  loadDir(parentPath(currentPath.value))
}

// ---- 右键菜单操作 ----
function onContextMenu(e: MouseEvent, file: FileNode) {
  e.preventDefault()
  menuTarget.value = file
  menuX.value = e.clientX
  menuY.value = e.clientY
  showMenu.value = true
}
function closeMenu() { showMenu.value = false }

function doUploadToRemote() {
  if (!menuTarget.value || menuTarget.value.is_dir) return
  closeMenu()
  emit('upload-request', menuTarget.value.path)
}

async function doNewFolder() {
  closeMenu()
  closeBlankMenu()
  const name = await promptDialog(t('prompt.folderName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）
  const final = await resolveNameConflict(name, new Set(files.value.map(f => f.name)))
  if (!final) return
  const path = currentPath.value.replace(/\\$/, '') + '\\' + final
  try {
    await invoke('create_local_dir', { path })
    loadDir(currentPath.value)
    showToast(t('toast.created', { name: final }), 'success')
  } catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

async function doNewFile() {
  closeMenu()
  closeBlankMenu()
  const name = await promptDialog(t('prompt.fileName'))
  if (!name) return
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突：询问自动改名/覆盖（系统文件管理器惯例）
  const final = await resolveNameConflict(name, new Set(files.value.map(f => f.name)))
  if (!final) return
  const path = currentPath.value.replace(/\\$/, '') + '\\' + final
  try {
    // 空内容创建（write_local_file 自动建父目录，此处父目录即当前目录）
    await invoke('write_local_file', { path, data: [] })
    loadDir(currentPath.value)
    showToast(t('toast.created', { name: final }), 'success')
  } catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

async function doRename() {
  if (!menuTarget.value) return
  closeMenu()
  const target = menuTarget.value
  const newName = await promptDialog(t('prompt.newName'), target.name)
  if (!newName || newName === target.name) return
  // 校验名称：拒绝路径分隔符与 ..（防移动到目录外）
  if (!isValidNewName(newName)) { showToast(t('toast.invalidName'), 'error', 4000); return }
  // 重名冲突（排除自身）：与新建一致的处理
  const others = new Set(files.value.filter(f => f.name !== target.name).map(f => f.name))
  const final = await resolveNameConflict(newName, others)
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
  <div class="file-tree" :class="{ 'drag-over': dragOver }" @drop.prevent="onDrop" @dragover="onDragover"
    @dragleave="onDragLeave" @click="closeMenu">
    <!-- VSCode EXPLORER 样式标题栏：标题居左，悬停显示新建文件/文件夹/刷新 -->
    <FileTreeHeader :title="t('tree.localTitle')" @new-file="doNewFile" @new-folder="doNewFolder" @refresh="refresh" />
    <div class="tree-body" @contextmenu="onBlankContextMenu">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="currentPath && parentPath(currentPath) !== currentPath" class="tree-node up-node" @click="goUp">
        <!-- 文件夹图标（琥珀色，SVG 无 emoji）：父目录 -->
        <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="#d29922" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
        <span class="name">..</span>
      </div>
      <div v-for="file in files" :key="file.path" class="tree-node" :class="{ selected: selected === file.path }"
        :draggable="!file.is_dir" @click="enterDir(file)" @dblclick="enterDir(file)"
        @dragstart="onDragStart($event, file)" @contextmenu="onContextMenu($event, file)">
        <!-- 文件夹/文件图标（SVG 无 emoji，项目约定）：文件夹琥珀色、文件中性灰 -->
        <svg v-if="file.is_dir" class="icon" viewBox="0 0 24 24" fill="none" stroke="#d29922" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
        <svg v-else class="icon" viewBox="0 0 24 24" fill="none" stroke="#8b949e" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>
        <span class="name">{{ file.name }}</span>
        <span v-if="!file.is_dir" class="size">{{ formatFileSize(file.size) }}</span>
      </div>
    </div>

    <!-- 右键菜单（文件/目录项，clamp 防溢出窗口边缘） -->
    <div v-if="showMenu" class="context-menu" :style="menuStyle">
      <div v-if="menuTarget && !menuTarget.is_dir" class="menu-item" @click="doUploadToRemote">{{ t('common.upload') }}
      </div>
      <div class="menu-item" @click="doNewFolder">{{ t('common.newFolder') }}</div>
      <div v-if="menuTarget" class="menu-item" @click="doRename">{{ t('common.rename') }}</div>
      <div v-if="menuTarget" class="menu-item" @click="doCopyPath">{{ t('common.copyPath') }}</div>
      <div v-if="menuTarget" class="menu-item" @click="doDelete">{{ t('common.delete') }}</div>
    </div>
    <!-- 空白区域右键菜单：新建文件/新建文件夹/刷新（VSCode 资源管理器对齐） -->
    <div v-if="blankMenu" class="context-menu" :style="blankMenuStyle" @click="closeBlankMenu">
      <div class="menu-item" @click="doNewFile">{{ t('common.newFile') }}</div>
      <div class="menu-item" @click="doNewFolder">{{ t('common.newFolder') }}</div>
      <div class="menu-item" @click="refresh">{{ t('common.refresh') }}</div>
    </div>
  </div>
</template>

<style scoped>
.file-tree {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--color-background-soft);
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
  background: var(--color-background-mute);
}

.tree-node.selected {
  background: var(--color-border-hover);
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
  outline: 2px solid hsla(160, 100%, 37%, 1);
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
