<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, promptDialog, showToast } from '../composables/dialog'
import { formatFileSize, isValidNewName } from '../composables/fs'
import { clampFloatPos } from '../composables/pos'
import { t } from '../composables/i18n'

// 自定义 MIME 类型：dragover 期间 getData() 不可读，只能读 types
const LOCAL_DRAG_TYPE = 'application/x-jackpot-local'
const REMOTE_DRAG_TYPE = 'application/x-jackpot-remote'

const props = defineProps<{ sessionId: string; refreshKey: number; locked?: boolean }>()

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
// 菜单位置 clamp（估算宽高，防贴窗口右缘/下缘时溢出被裁剪）
const menuStyle = computed(() => {
  const p = clampFloatPos(menuX.value, menuY.value, 140, 150)
  return { left: p.x + 'px', top: p.y + 'px' }
})
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
    loaded.value = true
    emit('current-dir', path)
  } catch (e) {
    if (seq !== loadSeq) return
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
  // 仅信任本应用内拖拽（自定义 MIME）：外部 text/plain 载荷不可信（可被伪造注入路径）
  const localPath = dt.getData(LOCAL_DRAG_TYPE)
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
  // 校验名称：拒绝路径分隔符与 ..（防移动到目录外）
  if (!isValidNewName(newName)) { showToast(t('toast.invalidName'), 'error', 4000); return }
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
  // 校验名称：拒绝路径分隔符与 ..（防建到目录外）
  if (!isValidNewName(name)) { showToast(t('toast.invalidName'), 'error', 4000); return }
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
// 解锁时自动刷新目录（传输结束后文件列表可能已变化）
watch(() => props.locked, (locked) => {
  if (!locked && props.sessionId) loadDir(currentPath.value)
})
</script>

<template>
  <div class="file-tree" :class="{ 'drag-over': dragOver, 'locked': locked }" @drop.prevent="onDrop" @dragover="onDragover" @dragleave="onDragLeave" @click="closeMenu">
    <div class="tree-header">
      {{ t('tree.remoteTitle') }}
      <span class="refresh" :title="t('common.refresh')" @click="refresh">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6"/>
        </svg>
      </span>
    </div>
    <!-- 锁定提示：锁图标 + 脉冲动画（与 SessionTab 状态条风格统一，SVG 无 emoji） -->
    <div v-if="locked" class="lock-banner">
      <svg class="lock-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />
      </svg>
      <span>{{ t('tree.transferLocked') }}</span>
    </div>
    <div class="tree-body">
      <div v-if="loading" class="loading">{{ t('common.loading') }}</div>
      <div v-if="error" class="error">{{ error }}</div>
      <div v-if="currentPath !== '/'" class="tree-node up-node" @click="goUp">
        <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
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
        <!-- 文件夹/文件图标（SVG 无 emoji，项目约定） -->
        <svg v-if="f.is_dir" class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>
        <svg v-else class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>
        <span class="name">{{ f.name }}</span>
        <span v-if="!f.is_dir" class="size">{{ formatFileSize(f.size) }}</span>
      </div>
    </div>

    <div v-if="showMenu" class="context-menu" :style="menuStyle">
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
