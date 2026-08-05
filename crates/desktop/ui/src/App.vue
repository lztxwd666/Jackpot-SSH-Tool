<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import Terminal from './components/Terminal.vue'
import LocalFileTree from './components/LocalFileTree.vue'
import RemoteFileTree from './components/RemoteFileTree.vue'
import TransferPanel from './components/TransferPanel.vue'
import ToastStack from './components/ToastStack.vue'
import SessionTab, { type SessionTabState } from './components/SessionTab.vue'
import { routeCoreEvent } from './composables/events'
import { dialogState, closeDialog, confirmDialog, showToast } from './composables/dialog'
import { t, getLocale, setLocale, type Locale } from './composables/i18n'

interface Host {
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

const hosts = ref<Host[]>([])
const searchQuery = ref('')
const status = ref('initializing...')

const editing = ref(false)
const selectedHost = ref<Host | null>(null)
const form = ref({
  name: '',
  address: '',
  port: 22,
  username: 'root',
  auth_type: 'password',
  group_name: '',
  favorite: false,
  notes: '',
})

const connecting = ref(false)
const connected = ref(false)
const channelId = ref('')
const sessionId = ref('')
const password = ref('')
const showPasswordPrompt = ref(false)
// 传输锁定时远程文件树锁定
const remoteTreeLocked = ref(false)
// 待确认主机密钥时的连接参数（确认后自动重连）
const pendingConnect = ref<null | { host: string; port: number; username: string; authType: string; password: string }>(null)

// 标签页工作区骨架：多会话标签模型（Task B2 接线 openSessionTab，旧单会话流程暂并存）
const tabs = ref<SessionTabState[]>([])
const activeTabId = ref<string | null>(null)

function activeTab() {
  return tabs.value.find(t => t.id === activeTabId.value) ?? null
}

function openSessionTab(sessionId: string, hostName: string, channelId: string, status: SessionTabState['status'] = 'connected') {
  // 已存在同主机连接中的标签 → 聚焦（不重复建连）
  const existing = tabs.value.find(t => t.hostName === hostName && t.status !== 'disconnected')
  if (existing) { activeTabId.value = existing.id; return existing.id }
  const tab: SessionTabState = { id: crypto.randomUUID(), hostName, sessionId, channelId, status }
  tabs.value.push(tab)
  activeTabId.value = tab.id
  return tab.id
}

function closeTab(tabId: string) {
  const tab = tabs.value.find(t => t.id === tabId)
  if (!tab) return
  if (tab.sessionId) {
    invoke('terminal_close', { sessionId: tab.sessionId }).catch(() => {})
  }
  tabs.value = tabs.value.filter(t => t.id !== tabId)
  if (activeTabId.value === tabId) {
    activeTabId.value = tabs.value.length ? tabs.value[tabs.value.length - 1].id : null
  }
}
// SFTP 相关状态
const localCurrentDir = ref('')       // 本地文件树当前目录（下载默认目标）
const remoteCurrentDir = ref('/')     // 远程文件树当前目录（上传默认目标）
const homeDir = ref('')               // 用户主目录（下载兜底目标）
const localRefreshKey = ref(0)        // 本地文件树刷新令牌
const remoteRefreshKey = ref(0)       // 远程文件树刷新令牌
const downloading = ref<Record<string, boolean>>({})  // 下载防重入守卫
const uploading = ref<Record<string, boolean>>({})    // 上传防重入守卫

// 传输任务进度（流式传输事件更新）
interface TransferTask {
  id: string
  name: string
  direction: 'download' | 'upload'
  done: number
  total: number
}
// 与后端 commands/sftp.rs 的 TransferProgress 结构对应
interface TransferProgress {
  id: string
  done: number
  total: number
}
const transfers = ref<Record<string, TransferTask>>({})

// 对话框输入框（prompt 模式）
const dialogInput = ref('')
watch(() => dialogState.visible, (v) => { if (v) dialogInput.value = dialogState.defaultValue })

// 界面语言（i18n）
const locale = ref<Locale>(getLocale())
function onLocaleChange(l: Locale) {
  setLocale(l)
  locale.value = l
}

// 主机 CRUD
async function loadHosts() {
  try { hosts.value = await invoke('list_hosts') } catch (e) { console.error('Failed to load hosts:', e) }
}
async function doSearch() {
  const q = searchQuery.value.trim()
  if (!q) { await loadHosts(); return }
  try { hosts.value = await invoke('search_hosts', { query: q }) } catch (e) { console.error('Search failed:', e) }
}
function newHost() {
  editing.value = true; selectedHost.value = null
  form.value = { name: '', address: '', port: 22, username: 'root', auth_type: 'password', group_name: '', favorite: false, notes: '' }
}
function editHost(host: Host) {
  editing.value = true; selectedHost.value = host
  form.value = { name: host.name, address: host.address, port: host.port, username: host.username, auth_type: host.auth_type, group_name: host.group_name, favorite: host.favorite, notes: host.notes }
}
async function saveHost() {
  try {
    const id = selectedHost.value ? selectedHost.value.id : crypto.randomUUID()
    await invoke('save_host', { host: { id, ...form.value, created_at: selectedHost.value?.created_at ?? '', updated_at: new Date().toISOString() } })
    editing.value = false; selectedHost.value = null; await loadHosts()
  } catch (e) { console.error('Save failed:', e) }
}
async function deleteHost(host: Host) {
  const ok = await confirmDialog(t('hosts.deleteConfirm', { name: host.name }))
  if (!ok) return
  try {
    await invoke('delete_host', { id: host.id })
    if (selectedHost.value?.id === host.id) {
      if (sessionId.value) { try { await invoke('terminal_close', { sessionId: sessionId.value }) } catch (_) {} }
      editing.value = false; selectedHost.value = null; connecting.value = false; connected.value = false; channelId.value = ''; sessionId.value = ''
    }
    await loadHosts()
  } catch (e) { console.error('Delete failed:', e) }
}
function cancelEdit() { editing.value = false; selectedHost.value = null }

function selectHost(host: Host) {
  if (sessionId.value && connected.value) { invoke('terminal_close', { sessionId: sessionId.value }).catch(() => {}) }
  selectedHost.value = host; editing.value = false; connected.value = false; connecting.value = false; channelId.value = ''; sessionId.value = ''
}

// 连接管理
function promptConnect() {
  if (!selectedHost.value) return
  if (selectedHost.value.auth_type === 'password') { showPasswordPrompt.value = true; password.value = '' }
  else { doConnect() }
}

async function doConnect() {
  if (!selectedHost.value) return
  if (sessionId.value) { try { await invoke('terminal_close', { sessionId: sessionId.value }) } catch (_) {} }
  connected.value = false; channelId.value = ''; sessionId.value = ''
  connecting.value = true; showPasswordPrompt.value = false
  // 保存连接参数：主机密钥确认后自动重连需要
  pendingConnect.value = {
    host: selectedHost.value.address,
    port: selectedHost.value.port,
    username: selectedHost.value.username,
    authType: selectedHost.value.auth_type,
    password: selectedHost.value.auth_type === 'password' ? password.value : '',
  }
  try {
    const sid = await invoke('create_session') as string; sessionId.value = sid
    await invoke('connect_session', {
      sessionId: sid, host: selectedHost.value.address, port: selectedHost.value.port,
      username: selectedHost.value.username, authType: selectedHost.value.auth_type,
      password: selectedHost.value.auth_type === 'password' ? password.value : null,
      privateKeyPath: null, privateKeyPassphrase: null,
    })
    pendingConnect.value = null
    // 连接成功后及时清空密码（减少在 JS 堆中的驻留时间）
    password.value = ''
    const cid = await invoke('open_shell', { sessionId: sid }) as string
    channelId.value = cid; connected.value = true
  } catch (e) {
    const isHostKeyError = String(e).includes('host key')
    // 主机密钥场景由 HostKey 事件驱动确认弹窗：不重复报错、不在控制台记录指纹
    if (isHostKeyError) {
      // 保留 pendingConnect 供 handleHostKey 确认后重连
    } else {
      console.error('Connect failed:', e)
      // 非主机密钥失败：清理待确认参数，避免陈旧状态
      pendingConnect.value = null
      showToast(t('toast.connectionFailed', { err: String(e) }), 'error', 5000)
    }
  }
  finally { connecting.value = false }
}

// 主机密钥确认：Unknown（首次连接）/ Changed（密钥变更，可能 MITM）
async function handleHostKey(kind: string, detail: any) {
  const host = detail?.host
  const fingerprint = kind === 'Changed' ? detail?.new_fingerprint : detail?.fingerprint
  const oldFp = detail?.old_fingerprint
  if (!host || !fingerprint || !pendingConnect.value) return
  const msg = kind === 'Changed'
    ? `Host key CHANGED for ${host}!\nOld: ${oldFp}\nNew: ${fingerprint}\n\nThis may indicate a man-in-the-middle attack. Trust the new key?`
    : `The authenticity of host '${host}' can't be established.\nFingerprint: ${fingerprint}\n\nTrust this host and continue connecting?`
  const ok = await confirmDialog(msg, kind === 'Changed' ? 'Warning: Host Key Changed' : 'Confirm Host Key')
  if (!ok) {
    // 拒绝信任：清理待确认参数与密码
    pendingConnect.value = null
    password.value = ''
    return
  }
  try {
    await invoke('approve_host_key', { host, port: pendingConnect.value.port, fingerprint })
    // 批准后自动重连
    const pc = pendingConnect.value
    pendingConnect.value = null
    password.value = pc.password
    await doConnect()
  } catch (e) {
    showToast('Failed to save host key: ' + e, 'error', 5000)
    pendingConnect.value = null
  }
}
function cancelConnect() { showPasswordPrompt.value = false; password.value = ''; connecting.value = false }

async function disconnect() {
  if (sessionId.value) { try { await invoke('terminal_close', { sessionId: sessionId.value }) } catch (_) {} }
  connected.value = false; channelId.value = ''; sessionId.value = ''
}

// SFTP 操作
// 下载目标优先级：拖拽目标目录 > 本地文件树当前目录 > 用户主目录\Downloads > 用户主目录
// 注意：C:\Users 根目录因 UAC 权限限制不可写，切勿作为默认目标
async function downloadFile(remotePath: string, localDir?: string) {
  if (!sessionId.value) return
  // 防重入：同一文件正在下载时忽略重复点击
  if (downloading.value[remotePath]) {
    showToast(t('toast.downloadInProgress'), 'info')
    return
  }
  downloading.value[remotePath] = true
  // 清洗文件名：替换 Windows 非法字符与路径分隔符，拒绝纯点（. 和 ..），防路径穿越
  const rawName = remotePath.split('/').pop() || 'download'
  const fileName = rawName.replace(/[\\/:*?"<>|]/g, '_').replace(/^\.+$/, '_')
  let dir = localDir || localCurrentDir.value
  if (!dir && homeDir.value) {
    dir = homeDir.value
    // 优先保存到 Downloads 目录（如果存在）
    try {
      await invoke('read_local_dir', { path: homeDir.value + '\\Downloads' })
      dir = homeDir.value + '\\Downloads'
    } catch (_) {}
  }
  const localPath = dir.replace(/\\$/, '') + '\\' + fileName

  // 创建传输任务（进度面板显示）
  const taskId = crypto.randomUUID()
  transfers.value[taskId] = { id: taskId, name: fileName, direction: 'download', done: 0, total: 0 }
  try {
    await invoke('sftp_download_file', {
      sessionId: sessionId.value, remotePath, localPath, taskId, expectedDir: dir,
    })
    // 刷新本地文件树
    localRefreshKey.value++
    showToast(t('toast.downloaded', { path: localPath }), 'success', 5000)
  } catch (e) {
    showToast(t('toast.downloadFailed', { err: String(e) }), 'error', 5000)
  } finally {
    downloading.value[remotePath] = false
    // 完成后短暂保留进度条（显示 100%），随后移除
    setTimeout(() => { delete transfers.value[taskId] }, 1500)
  }
}

async function uploadFile(remoteDir: string, localPath: string) {
  if (!sessionId.value) return
  // 防重入：同一文件正在上传时忽略重复操作
  if (uploading.value[localPath]) {
    showToast(t('toast.uploadInProgress'), 'info')
    return
  }
  uploading.value[localPath] = true
  const fileName = localPath.split('\\').pop() || 'upload'
  const remotePath = remoteDir.replace(/\/$/, '') + '/' + fileName

  // 创建传输任务
  const taskId = crypto.randomUUID()
  transfers.value[taskId] = { id: taskId, name: fileName, direction: 'upload', done: 0, total: 0 }
  try {
    await invoke('sftp_upload_file', {
      sessionId: sessionId.value, remotePath, localPath, taskId,
      expectedDir: localCurrentDir.value || homeDir.value || '',
    })
    // 触发远程文件树刷新
    remoteRefreshKey.value++
    showToast(t('toast.uploaded', { path: remotePath }), 'success', 5000)
  } catch (e) {
    showToast(t('toast.uploadFailed', { err: String(e) }), 'error', 5000)
  } finally {
    uploading.value[localPath] = false
    setTimeout(() => { delete transfers.value[taskId] }, 1500)
  }
}

// 本地文件树右键 "Upload to Remote"：上传到远程当前目录
async function uploadFromLocal(localPath: string) {
  if (!sessionId.value) {
    showToast(t('toast.notConnected'), 'error')
    return
  }
  uploadFile(remoteCurrentDir.value || '/', localPath)
}

let unlistenCore: () => void
let unlistenTransfer: () => void

onMounted(async () => {
  // 查询真实运行时状态（不再硬编码 running）
  try {
    const appStatus = await invoke('get_app_status') as string
    status.value = appStatus === 'running' ? 'running' : 'stopped'
  } catch (_) { status.value = 'unknown' }
  await loadHosts()
  // 获取用户主目录（下载兜底目标）
  try { homeDir.value = await invoke('get_home_dir') } catch (_) {}
  unlistenCore = await listen<string>('core-event', (event) => {
    const parsed = JSON.parse(event.payload)
    // 非会话级事件（不带 session_id）：仍在本地处理
    if (parsed.type === 'Host') loadHosts()
    // 主机密钥确认流程：Unknown（首次）/ Changed（变更）
    if (parsed.type === 'HostKey' && (parsed.payload?.kind === 'Unknown' || parsed.payload?.kind === 'Changed')) {
      handleHostKey(parsed.payload.kind, parsed.payload?.detail)
    }
    // 会话级事件：按 session_id 路由到对应标签
    routeCoreEvent(event.payload, {
      onSession: (sid, kind, detail) => {
        const tab = tabs.value.find(t => t.sessionId === sid)
        if (!tab) return
        if (kind === 'Connected') { tab.status = 'connected'; tab.error = undefined }
        // Disconnected/Connecting 等状态：Task B3 完整接线（本任务先建骨架）
      },
      onTransfer: (sid, kind) => {
        // 旧单会话视图沿用：传输窗口标记锁定远程文件树
        // Task B3 按 tab 粒度接线传输锁状态
        remoteTreeLocked.value = kind === 'Locked'
      },
    })
  })
  // 传输进度事件（流式下载/上传）
  // 与后端 commands/sftp.rs 的 TransferProgress 结构对应
  unlistenTransfer = await listen<TransferProgress>('transfer-progress', (event) => {
    const tr = transfers.value[event.payload.id]
    if (tr) {
      tr.done = event.payload.done
      tr.total = event.payload.total
    }
  })
})

onBeforeUnmount(() => {
  unlistenCore?.()
  unlistenTransfer?.()
})
</script>

<template>
  <div class="app-layout">
    <!-- 左侧区域：标签栏 + 工作区 + 状态栏（右侧主机栏完全独立） -->
    <div class="left-area">
      <!-- 标签栏：flex-wrap 换行展示，不做横向滚动 -->
      <div class="tab-bar">
        <div v-for="tab in tabs" :key="tab.id" class="tab" :class="{ active: tab.id === activeTabId }" @click="activeTabId = tab.id">
          <span class="tab-dot" :class="tab.status"></span>
          <span class="tab-name">{{ tab.hostName }}</span>
          <span class="tab-close" @click.stop="closeTab(tab.id)">×</span>
        </div>
      </div>
      <div class="tab-content">
        <!-- 新标签骨架：按 session_id 路由，v-show 切换保持各标签组件状态 -->
        <template v-for="tab in tabs" :key="tab.id">
          <SessionTab v-show="tab.id === activeTabId" :tab="tab" @close="closeTab(tab.id)" />
        </template>
        <!-- 旧单会话视图（Task 6 迁移到标签后删除）：无标签时沿用旧连接流程 -->
        <template v-if="tabs.length === 0">
        <!-- 本地文件树 -->
        <div v-if="connected" class="panel" style="width:180px; min-width:180px;">
          <LocalFileTree
            :refreshKey="localRefreshKey"
            @download="downloadFile"
            @current-dir="(p: string) => localCurrentDir = p"
            @upload-request="uploadFromLocal"
          />
        </div>

        <!-- 远程文件树 -->
        <div v-if="connected" class="panel" style="width:180px; min-width:180px;">
          <RemoteFileTree
            :sessionId="sessionId"
            :refreshKey="remoteRefreshKey"
            :locked="remoteTreeLocked"
            @download="downloadFile"
            @upload="uploadFile"
            @current-dir="(p: string) => remoteCurrentDir = p"
          />
        </div>

        <!-- 终端 -->
        <main class="main-panel">
      <template v-if="connected && channelId">
        <div class="terminal-wrapper">
          <div class="terminal-header">
            <span class="connection-info">
              {{ selectedHost?.name }} ({{ selectedHost?.username }}@{{ selectedHost?.address }}:{{ selectedHost?.port }})
            </span>
            <button class="btn btn-danger" @click="disconnect">{{ t('common.disconnect') }}</button>
          </div>
          <Terminal :channelId="channelId" :key="channelId" />
        </div>
      </template>
      <template v-else-if="editing">
        <div class="form-header"><h3>{{ selectedHost ? t('hosts.editHost') : t('hosts.newHost') }}</h3></div>
        <form class="host-form" @submit.prevent="saveHost">
          <label>{{ t('form.name') }} <input v-model="form.name" type="text" required placeholder="My Server" /></label>
          <label>{{ t('form.address') }} <input v-model="form.address" type="text" required placeholder="192.168.1.1" /></label>
          <label>{{ t('form.port') }} <input v-model.number="form.port" type="number" required min="1" max="65535" /></label>
          <label>{{ t('form.username') }} <input v-model="form.username" type="text" required placeholder="root" /></label>
          <label>{{ t('form.authType') }}
            <select v-model="form.auth_type">
              <option value="password">{{ t('form.authPassword') }}</option>
              <option value="private_key">{{ t('form.authPrivateKey') }}</option>
              <option value="agent">{{ t('form.authAgent') }}</option>
            </select>
          </label>
          <label>{{ t('form.group') }} <input v-model="form.group_name" type="text" placeholder="Production" /></label>
          <label>{{ t('form.notes') }} <textarea v-model="form.notes" rows="3" placeholder="Optional notes..."></textarea></label>
          <label class="checkbox-label"><input v-model="form.favorite" type="checkbox" /> {{ t('form.favorite') }}</label>
          <div class="form-actions">
            <button type="submit" class="btn btn-primary">{{ t('common.save') }}</button>
            <button type="button" class="btn" @click="cancelEdit">{{ t('common.cancel') }}</button>
            <button v-if="selectedHost" type="button" class="btn btn-danger" @click="deleteHost(selectedHost!)">{{ t('common.delete') }}</button>
          </div>
        </form>
      </template>
      <template v-else-if="selectedHost">
        <div class="host-detail">
          <div class="host-detail-header">
            <h3>{{ selectedHost.name }}</h3>
            <div class="host-detail-actions">
              <button class="btn btn-primary" @click="promptConnect" :disabled="connecting">{{ connecting ? t('common.connecting') : t('common.connect') }}</button>
              <button class="btn" @click="editHost(selectedHost!)">{{ t('common.edit') }}</button>
            </div>
          </div>
          <div class="host-detail-info">
            <div class="info-row"><span class="info-label">{{ t('detail.address') }}</span><span class="info-value">{{ selectedHost.address }}:{{ selectedHost.port }}</span></div>
            <div class="info-row"><span class="info-label">{{ t('detail.username') }}</span><span class="info-value">{{ selectedHost.username }}</span></div>
            <div class="info-row"><span class="info-label">{{ t('detail.auth') }}</span><span class="info-value">{{ selectedHost.auth_type }}</span></div>
            <div class="info-row" v-if="selectedHost.group_name"><span class="info-label">{{ t('detail.group') }}</span><span class="info-value">{{ selectedHost.group_name }}</span></div>
            <div class="info-row" v-if="selectedHost.notes"><span class="info-label">{{ t('detail.notes') }}</span><span class="info-value">{{ selectedHost.notes }}</span></div>
          </div>
        </div>
      </template>
      <template v-else>
        <div class="placeholder"><p>{{ t('hosts.selectHint') }}</p></div>
      </template>
        </main>
        </template>
      </div>
      <!-- 状态栏：左侧区域底部，展示运行状态（后续会话信息由 Task B3 完善） -->
      <div class="status-bar">
        <span class="status-badge">{{ status }}</span>
        <!-- 后续状态显示预留位 -->
      </div>
    </div>

    <!-- 右侧主机栏（完整独立，Task B2 重构为 HostPanel） -->
    <aside class="sidebar">
      <div class="sidebar-header"><h2>{{ t('hosts.title') }}</h2><button class="btn btn-primary" @click="newHost">{{ t('hosts.add') }}</button></div>
      <div class="search-bar"><input v-model="searchQuery" type="text" :placeholder="t('hosts.searchPlaceholder')" @input="doSearch" /></div>
      <ul class="host-list">
        <li v-for="host in hosts" :key="host.id" :class="{ active: selectedHost?.id === host.id }" @click="selectHost(host)">
          <span class="host-name">{{ host.name }}</span>
          <span class="host-addr">{{ host.address }}:{{ host.port }}</span>
        </li>
      </ul>
      <div class="sidebar-footer">
        <select class="locale-select" :value="locale" @change="onLocaleChange(($event.target as HTMLSelectElement).value as Locale)">
          <option value="en">English</option>
          <option value="zh">中文</option>
        </select>
      </div>
    </aside>

    <!-- 全局确认/输入对话框（带确认 + 取消，可反悔） -->
    <div v-if="dialogState.visible" class="modal-overlay" @click.self="closeDialog(dialogState.mode === 'prompt' ? null : false)">
      <div class="modal">
        <h3>{{ dialogState.title }}</h3>
        <p>{{ dialogState.message }}</p>
        <form v-if="dialogState.mode === 'prompt'" @submit.prevent="closeDialog(dialogInput)">
          <input v-model="dialogInput" type="text" autofocus />
          <div class="modal-actions">
            <button type="submit" class="btn btn-primary">{{ t('common.ok') }}</button>
            <button type="button" class="btn" @click="closeDialog(null)">{{ t('common.cancel') }}</button>
          </div>
        </form>
        <div v-else class="modal-actions">
          <button class="btn btn-primary" @click="closeDialog(true)">{{ t('common.ok') }}</button>
          <button class="btn" @click="closeDialog(false)">{{ t('common.cancel') }}</button>
        </div>
      </div>
    </div>

    <!-- 全局 Toast 提示栈 -->
    <ToastStack />

    <!-- 传输进度面板 -->
    <TransferPanel :transfers="transfers" />

    <!-- 密码弹窗 -->
    <div v-if="showPasswordPrompt" class="modal-overlay" @click.self="cancelConnect">
      <div class="modal">
        <h3>{{ t('hosts.enterPassword') }}</h3>
        <p>{{ t('hosts.connectingTo', { user: selectedHost?.username || '', host: selectedHost?.address || '' }) }}</p>
        <form @submit.prevent="doConnect">
          <input v-model="password" type="password" :placeholder="t('hosts.passwordPlaceholder')" autofocus required />
          <div class="modal-actions">
            <button type="submit" class="btn btn-primary" :disabled="connecting">{{ connecting ? t('common.connecting') : t('common.connect') }}</button>
            <button type="button" class="btn" @click="cancelConnect">{{ t('common.cancel') }}</button>
          </div>
        </form>
      </div>
    </div>

  </div>
</template>

<style scoped>
.app-layout { display: flex; height: 100vh; width: 100vw; overflow: hidden; }
.panel { display: flex; flex-direction: column; overflow: hidden; }

.sidebar {
  width: 220px; min-width: 220px; background: var(--color-background-soft);
  border-left: 1px solid var(--color-border); display: flex; flex-direction: column;
}
.sidebar-header { display: flex; justify-content: space-between; align-items: center; padding: 0.6rem 0.8rem; border-bottom: 1px solid var(--color-border); }
.sidebar-header h2 { font-size: 0.95rem; color: var(--color-heading); }
.search-bar { padding: 0.4rem 0.6rem; border-bottom: 1px solid var(--color-border); }
.search-bar input { width: 100%; padding: 0.3rem 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background); color: var(--color-text); font-size: 0.8rem; }
.host-list { flex: 1; overflow-y: auto; list-style: none; padding: 0; margin: 0; }
.host-list li { padding: 0.5rem 0.8rem; cursor: pointer; border-bottom: 1px solid var(--color-border); }
.host-list li:hover { background: var(--color-background-mute); }
.host-list li.active { background: var(--color-border-hover); }
.host-name { display: block; font-weight: 600; color: var(--color-heading); font-size: 0.85rem; }
.host-addr { font-size: 0.7rem; color: var(--color-text); opacity: 0.7; }
.sidebar-footer { padding: 0.4rem 0.6rem; border-top: 1px solid var(--color-border); display: flex; align-items: center; justify-content: space-between; gap: 0.4rem; }
.status-badge { font-size: 0.7rem; color: hsla(160, 100%, 37%, 1); }
.locale-select {
  background: var(--color-background); color: var(--color-text);
  border: 1px solid var(--color-border); border-radius: 4px;
  font-size: 0.7rem; padding: 0.1rem 0.2rem; cursor: pointer;
}

.main-panel { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.form-header h3 { margin-bottom: 1rem; color: var(--color-heading); padding: 1rem; }
.host-form { max-width: 400px; display: flex; flex-direction: column; gap: 0.8rem; padding: 0 1rem; }
.host-form label { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.8rem; color: var(--color-text); }
.host-form input, .host-form select, .host-form textarea {
  padding: 0.4rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); font-size: 0.85rem; font-family: inherit;
}
.host-form textarea { resize: vertical; }
.checkbox-label { flex-direction: row !important; align-items: center; gap: 0.4rem !important; }
.form-actions { display: flex; gap: 0.4rem; margin-top: 0.4rem; }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.btn-primary { background: hsla(160, 100%, 37%, 1); color: #fff; border-color: hsla(160, 100%, 37%, 1); }
.btn-primary:hover { background: hsla(160, 100%, 30%, 1); }
.btn-danger { color: #e5534b; border-color: #e5534b; }
.btn-danger:hover { background: #e5534b; color: #fff; }

.placeholder { display: flex; align-items: center; justify-content: center; height: 100%; color: var(--color-text); opacity: 0.5; }
.host-detail { padding: 1.5rem; }
.host-detail-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
.host-detail-header h3 { color: var(--color-heading); font-size: 1.2rem; }
.host-detail-actions { display: flex; gap: 0.4rem; }
.host-detail-info { display: flex; flex-direction: column; gap: 0.6rem; }
.info-row { display: flex; gap: 0.8rem; align-items: baseline; }
.info-label { font-size: 0.75rem; color: var(--color-text); opacity: 0.6; min-width: 70px; }
.info-value { font-size: 0.85rem; color: var(--color-text); }

.terminal-wrapper { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.terminal-header { display: flex; justify-content: space-between; align-items: center; padding: 0.4rem 0.8rem; background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); flex-shrink: 0; }
.connection-info { font-size: 0.8rem; color: var(--color-text); font-weight: 500; }

/* 标签页工作区布局：左侧区域三行（标签栏/工作区/状态栏），右侧主机栏独立 */
.left-area { flex: 1; display: flex; flex-direction: column; min-width: 0; }
.tab-bar { display: flex; flex-wrap: wrap; /* 标签换行，不做横向滚动 */ background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); padding: 0.2rem 0.2rem 0; gap: 0.2rem; }
.tab { display: inline-flex; align-items: center; gap: 0.4rem; padding: 0.3rem 0.6rem; background: var(--color-background); border: 1px solid var(--color-border); border-radius: 4px 4px 0 0; cursor: pointer; font-size: 0.8rem; }
.tab.active { border-bottom-color: var(--color-background); background: var(--color-background-mute); }
.tab-dot { width: 8px; height: 8px; border-radius: 50%; }
.tab-dot.connected { background: hsla(160, 100%, 37%, 1); }
.tab-dot.connecting, .tab-dot.reconnecting { background: #d29922; }
.tab-dot.disconnected { background: #e5534b; }
.tab-close { color: var(--color-text); opacity: 0.6; cursor: pointer; }
.tab-close:hover { opacity: 1; }
.tab-content { flex: 1; display: flex; overflow: hidden; }
.status-bar { padding: 0.3rem 0.6rem; border-top: 1px solid var(--color-border); font-size: 0.7rem; }

.modal-overlay { position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: var(--color-background); border: 1px solid var(--color-border); border-radius: 8px; padding: 1.5rem; min-width: 300px; box-shadow: 0 4px 24px rgba(0,0,0,0.3); }
.modal h3 { color: var(--color-heading); margin-bottom: 0.5rem; }
.modal p { color: var(--color-text); opacity: 0.7; font-size: 0.8rem; margin-bottom: 1rem; }
.modal input { width: 100%; padding: 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background-soft); color: var(--color-text); font-size: 0.85rem; margin-bottom: 0.8rem; box-sizing: border-box; }
.modal-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }

</style>
