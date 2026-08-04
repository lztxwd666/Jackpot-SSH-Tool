<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
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

// 右键菜单状态
const showMenu = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuTarget = ref<FileNode | null>(null)

function parentPath(p: string): string {
  const stripped = p.replace(/\\$/, '')
  if (stripped.length === 2 && stripped.endsWith(':')) return ''
  const idx = Math.max(stripped.lastIndexOf('\\'), stripped.lastIndexOf('/'))
  if (idx < 0) return ''
  const parent = stripped.substring(0, idx)
  if (parent.length === 2 && parent.endsWith(':')) return parent + '\\'
  return parent
}

async function loadDir(path: string) {
  loading.value = true
  try {
    files.value = await invoke('read_local_dir', { path })
    currentPath.value = path
    emit('current-dir', path)
  } catch (e) {
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
  let remotePath = dt.getData(REMOTE_DRAG_TYPE)
  if (!remotePath) {
    const tp = dt.getData('text/plain') || ''
    if (tp.startsWith('remote:')) remotePath = tp.slice(7)
  }
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
  const name = await promptDialog(t('prompt.folderName'))
  if (!name) return
  const path = currentPath.value.replace(/\\$/, '') + '\\' + name
  try {
    await invoke('create_local_dir', { path })
    loadDir(currentPath.value)
    showToast(t('toast.created', { name }), 'success')
  } catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}

async function doRename() {
  if (!menuTarget.value) return
  closeMenu()
  const target = menuTarget.value
  const newName = await promptDialog(t('prompt.newName'), target.name)
  if (!newName || newName === target.name) return
  const parent = currentPath.value.replace(/\\$/, '')
  try {
    await invoke('rename_local_file', { oldPath: target.path, newPath: parent + '\\' + newName })
    loadDir(currentPath.value)
    showToast(t('toast.renamed', { name: newName }), 'success')
  } catch (e) { showToast(t('toast.renameFailed', { err: String(e) }), 'error', 5000) }
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

onMounted(() => loadDir(''))

// 外部刷新令牌（下载完成后自动刷新当前目录）
watch(() => props.refreshKey, () => { loadDir(currentPath.value) })
</script>

<template>
  <div class="file-tree" :class="{ 'drag-over': dragOver }" @drop.prevent="onDrop" @dragover="onDragover"
    @dragleave="onDragLeave" @click="closeMenu">
    <div class="tree-header">{{ t('tree.localTitle') }}</div>
    <div class="tree-body">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="currentPath && parentPath(currentPath) !== currentPath" class="tree-node up-node" @click="goUp">
        <span class="icon">📂</span>
        <span class="name">..</span>
      </div>
      <div v-for="file in files" :key="file.path" class="tree-node" :class="{ selected: selected === file.path }"
        :draggable="!file.is_dir" @click="enterDir(file)" @dblclick="enterDir(file)"
        @dragstart="onDragStart($event, file)" @contextmenu="onContextMenu($event, file)">
        <span class="icon">{{ file.is_dir ? '📁' : '📄' }}</span>
        <span class="name">{{ file.name }}</span>
        <span v-if="!file.is_dir" class="size">{{ (file.size / 1024).toFixed(0) }}K</span>
      </div>
    </div>

    <!-- 右键菜单 -->
    <div v-if="showMenu" class="context-menu" :style="{ left: menuX + 'px', top: menuY + 'px' }">
      <div v-if="menuTarget && !menuTarget.is_dir" class="menu-item" @click="doUploadToRemote">{{ t('common.upload') }}
      </div>
      <div class="menu-item" @click="doNewFolder">{{ t('common.newFolder') }}</div>
      <div v-if="menuTarget" class="menu-item" @click="doRename">{{ t('common.rename') }}</div>
      <div v-if="menuTarget" class="menu-item" @click="doDelete">{{ t('common.delete') }}</div>
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
