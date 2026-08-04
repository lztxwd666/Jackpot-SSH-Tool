<script setup lang="ts">
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
import { t } from '../composables/i18n'

// 自定义 MIME 类型：dragover 期间 getData() 不可读，只能读 types
const LOCAL_DRAG_TYPE = 'application/x-jackpot-local'
const REMOTE_DRAG_TYPE = 'application/x-jackpot-remote'

const props = defineProps<{ sessionId: string; refreshKey: number }>()

const emit = defineEmits<{
  (e: 'download', remotePath: string): void
  (e: 'upload', remoteDir: string, localPath: string): void
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
const selected = ref('')
const loading = ref(false)
const error = ref('')
const showMenu = ref(false)
const menuX = ref(0)
const menuY = ref(0)
const menuTarget = ref<FileNode | null>(null)
const loaded = ref(false)
const dragOver = ref(false)  // 拖拽悬停高亮

function parentPath(p: string): string {
  if (p === '/') return '/'
  const parts = p.replace(/\/$/, '').split('/').filter(Boolean)
  parts.pop()
  return '/' + parts.join('/')
}

async function loadDir(path: string) {
  if (!props.sessionId) return
  loading.value = true; error.value = ''
  try {
    files.value = await invoke('sftp_list_dir', { sessionId: props.sessionId, path })
    currentPath.value = path
    loaded.value = true
    emit('current-dir', path)
  } catch (e) {
    error.value = String(e)
    console.error('sftp_list_dir failed:', e)
  }
  loading.value = false
}

function enterDir(f: FileNode) {
  selected.value = f.path
  if (f.is_dir) loadDir(f.path)
}

function goUp() {
  loadDir(parentPath(currentPath.value))
}

function refresh() { loadDir(currentPath.value) }

// ---- 拖拽：远程文件拖出（下载到本地） ----
function onDragStart(e: DragEvent, f: FileNode) {
  if (f.is_dir) return
  const dt = e.dataTransfer!
  dt.setData(REMOTE_DRAG_TYPE, f.path)
  dt.setData('text/plain', 'remote:' + f.path)
  dt.effectAllowed = 'copy'
}

// 本地文件拖入（上传）：无条件接受悬停，drop 时校验数据
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
  let localPath = dt.getData(LOCAL_DRAG_TYPE)
  if (!localPath) {
    const tp = dt.getData('text/plain') || ''
    if (tp.startsWith('local:')) localPath = tp.slice(6)
  }
  if (localPath) {
    emit('upload', currentPath.value, localPath)
  }
}

// 右键菜单
function onContextMenu(e: MouseEvent, f: FileNode) {
  e.preventDefault()
  menuTarget.value = f
  menuX.value = e.clientX
  menuY.value = e.clientY
  showMenu.value = true
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
  const parent = currentPath.value.replace(/\/$/, '')
  try {
    await invoke('sftp_rename', { sessionId: props.sessionId, oldPath: target.path, newPath: parent + '/' + newName })
    loadDir(currentPath.value)
    showToast(t('toast.renamed', { name: newName }), 'success')
  } catch (e) { showToast(t('toast.renameFailed', { err: String(e) }), 'error', 5000) }
}
async function doNewFolder() {
  if (!props.sessionId) return; closeMenu()
  const name = await promptDialog(t('prompt.folderName'))
  if (!name) return
  const path = currentPath.value.replace(/\/$/, '') + '/' + name
  try {
    await invoke('sftp_create_dir', { sessionId: props.sessionId, path })
    loadDir(currentPath.value)
    showToast(t('toast.created', { name }), 'success')
  }
  catch (e) { showToast(t('toast.createFailed', { err: String(e) }), 'error', 5000) }
}
function doDownload() {
  if (!menuTarget.value || menuTarget.value.is_dir) return
  closeMenu(); emit('download', menuTarget.value.path)
}

// sessionId 变化或刷新令牌变化时重新加载
watch(() => props.sessionId, (sid) => { if (sid) loadDir('/') }, { immediate: true })
watch(() => props.refreshKey, () => { if (props.sessionId) loadDir(currentPath.value) })
</script>

<template>
  <div class="file-tree" :class="{ 'drag-over': dragOver }" @drop.prevent="onDrop" @dragover="onDragover" @dragleave="onDragLeave" @click="closeMenu">
    <div class="tree-header">
      {{ t('tree.remoteTitle') }}
      <span class="refresh" :title="t('common.refresh')" @click="refresh">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6"/>
        </svg>
      </span>
    </div>
    <div class="tree-body">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="error" class="error">{{ error }}</div>
      <div v-if="currentPath !== '/'" class="tree-node up-node" @click="goUp">
        <span class="icon">📂</span>
        <span class="name">..</span>
      </div>
      <div
        v-for="f in files"
        :key="f.path"
        class="tree-node"
        :class="{ selected: selected === f.path }"
        :draggable="!f.is_dir"
        @click="enterDir(f)"
        @dblclick="enterDir(f)"
        @dragstart="onDragStart($event, f)"
        @contextmenu="onContextMenu($event, f)"
      >
        <span class="icon">{{ f.is_dir ? '📁' : '📄' }}</span>
        <span class="name">{{ f.name }}</span>
        <span v-if="!f.is_dir" class="size">{{ (f.size / 1024).toFixed(0) }}K</span>
      </div>
    </div>

    <div v-if="showMenu" class="context-menu" :style="{ left: menuX + 'px', top: menuY + 'px' }">
      <div v-if="menuTarget && !menuTarget.is_dir" class="menu-item" @click="doDownload">{{ t('common.download') }}</div>
      <div class="menu-item" @click="doDelete">{{ t('common.delete') }}</div>
      <div class="menu-item" @click="doRename">{{ t('common.rename') }}</div>
      <div class="menu-item" @click="doNewFolder">{{ t('common.newFolder') }}</div>
    </div>
  </div>
</template>

<style scoped>
.file-tree {
  display: flex; flex-direction: column; height: 100%;
  background: var(--color-background-soft); border-left: 1px solid var(--color-border);
  user-select: none; font-size: 0.8rem; position: relative;
}
.tree-header {
  padding: 0.4rem 0.5rem; font-weight: 600; color: var(--color-heading);
  border-bottom: 1px solid var(--color-border); text-align: center; flex-shrink: 0;
}
.tree-body { flex: 1; overflow-y: auto; }
.tree-node {
  display: flex; align-items: center; gap: 0.3rem; padding: 0.2rem 0.5rem;
  cursor: pointer; white-space: nowrap;
}
.tree-node:hover { background: var(--color-background-mute); }
.tree-node.selected { background: var(--color-border-hover); }
.up-node { border-bottom: 1px solid var(--color-border); font-weight: 500; }
.icon { flex-shrink: 0; }
.name { overflow: hidden; text-overflow: ellipsis; flex: 1; }
.size { font-size: 0.7rem; opacity: 0.5; flex-shrink: 0; }
.loading { padding: 0.5rem; opacity: 0.5; text-align: center; }
.error { padding: 0.5rem; color: #e5534b; font-size: 0.75rem; text-align: center; }
.drag-over {
  outline: 2px solid hsla(160, 100%, 37%, 1);
  outline-offset: -2px;
  background: rgba(46, 160, 67, 0.08);
}
.refresh {
  cursor: pointer; color: var(--color-text); opacity: 0.6;
  display: inline-flex; align-items: center; margin-left: 6px;
  transition: opacity 0.15s;
}
.refresh:hover { opacity: 1; }
.context-menu {
  position: fixed; background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 4px; box-shadow: 0 2px 8px rgba(0,0,0,0.3); z-index: 1000; min-width: 120px;
}
.menu-item { padding: 0.3rem 0.8rem; cursor: pointer; font-size: 0.8rem; }
.menu-item:hover { background: var(--color-background-mute); }
</style>
