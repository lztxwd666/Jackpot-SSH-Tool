<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import HostPanel, { type Host } from './components/HostPanel.vue'
import HostFormPanel, { type HostForm } from './components/HostFormPanel.vue'
import TransferPanel from './components/TransferPanel.vue'
import ToastStack from './components/ToastStack.vue'
import SessionTab, { type SessionTabState } from './components/SessionTab.vue'
import { routeCoreEvent } from './composables/events'
import { dialogState, closeDialog, confirmDialog, showToast } from './composables/dialog'
import { t, getLocale, setLocale, type Locale } from './composables/i18n'

// 与后端 commands/host.rs 的 PingResult 结构对应
interface PingResult {
  success: boolean
  latency_ms: number | null
}

const hosts = ref<Host[]>([])
const searchQuery = ref('')
const status = ref('initializing...')

// 双击直连状态：密码弹框（保存密码勾选由 Task B4 接线，暂无条件判断）
const connecting = ref(false)
const password = ref('')
const showPasswordPrompt = ref(false)
// 待确认主机密钥时的连接参数（确认后自动重连）
const pendingConnectHost = ref<null | { host: Host; password: string }>(null)

// 标签页工作区：多会话标签模型（旧单会话视图已在 Task 6 删除）
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

// 滑出表单面板编排：'none' 关闭 / 'new' 新增 / 'edit' 编辑
const panelOpen = ref<'none' | 'new' | 'edit'>('none')
const editingHost = ref<Host | null>(null)

function openNewPanel() { panelOpen.value = 'new'; editingHost.value = null }
function openEditPanel(host: Host) { panelOpen.value = 'edit'; editingHost.value = host }
function cancelPanel() { panelOpen.value = 'none'; editingHost.value = null }

// Host → 表单初始值（编辑时不回显密码；Task B4 接线后按保存勾选回显）
function toForm(host: Host): HostForm {
  return {
    id: host.id, name: host.name, address: host.address, port: host.port,
    username: host.username, auth_type: host.auth_type, group_name: host.group_name,
    favorite: host.favorite, notes: host.notes, password: '', save_password: false,
  }
}

// 表单保存：新增或更新走现有 save_host 流程（凭据保存由 Task B4 接线）
async function saveHostForm(form: HostForm) {
  try {
    const existing = form.id ? hosts.value.find(h => h.id === form.id) : undefined
    await invoke('save_host', {
      host: {
        id: form.id ?? crypto.randomUUID(),
        name: form.name, address: form.address, port: form.port,
        username: form.username, auth_type: form.auth_type, group_name: form.group_name,
        favorite: form.favorite, notes: form.notes,
        created_at: existing?.created_at ?? '',
        updated_at: new Date().toISOString(),
      },
    })
    cancelPanel()
    await loadHosts()
  } catch (e) { console.error('Save failed:', e) }
}

// 主机删除：确认后删除主机，并连带关闭该主机的标签（凭据删除由 Task B4 接线）
async function onDeleteHost(host: Host) {
  const ok = await confirmDialog(t('hosts.deleteConfirm', { name: host.name }))
  if (!ok) return
  try {
    await invoke('delete_host', { id: host.id })
    for (const tab of [...tabs.value]) {
      if (tab.hostName === host.name) closeTab(tab.id)
    }
    await loadHosts()
  } catch (e) { console.error('Delete failed:', e) }
}

// 主机 CRUD 与搜索
async function loadHosts() {
  try { hosts.value = await invoke('list_hosts') } catch (e) { console.error('Failed to load hosts:', e) }
}
async function doSearch() {
  const q = searchQuery.value.trim()
  if (!q) { await loadHosts(); return }
  try { hosts.value = await invoke('search_hosts', { query: q }) } catch (e) { console.error('Search failed:', e) }
}
function onSearch(q: string) {
  searchQuery.value = q
  doSearch()
}

// 双击直连流程：复用密码弹框/主机密钥确认逻辑，改为创建/聚焦标签
async function connectHost(host: Host) {
  // 防重入：连接进行中忽略新的双击
  if (connecting.value) return
  // 同主机已有活动标签：聚焦而非重复建连
  const existing = tabs.value.find(t => t.hostName === host.name && t.status !== 'disconnected')
  if (existing) { activeTabId.value = existing.id; return }
  if (host.auth_type === 'password') {
    // 未保存密码：弹密码框，确认后执行直连（保存密码勾选由 Task B4 接线）
    pendingConnectHost.value = { host, password: '' }
    showPasswordPrompt.value = true
    password.value = ''
    return
  }
  await doConnectWith(host, null)
}

// 执行连接：create_session → connect_session → open_shell → 打开标签
async function doConnectWith(host: Host, secret: string | null) {
  connecting.value = true
  try {
    const sid = await invoke('create_session') as string
    // 保存连接参数：主机密钥确认后自动重连需要
    pendingConnectHost.value = { host, password: secret ?? '' }
    await invoke('connect_session', {
      sessionId: sid, host: host.address, port: host.port,
      username: host.username, authType: host.auth_type,
      password: secret, privateKeyPath: null, privateKeyPassphrase: null,
    })
    // 连接成功后及时清空密码（减少在 JS 堆中的驻留时间）
    password.value = ''
    pendingConnectHost.value = null
    const cid = await invoke('open_shell', { sessionId: sid }) as string
    openSessionTab(sid, host.name, cid)
  } catch (e) {
    const isHostKeyError = String(e).includes('host key')
    // 主机密钥场景由 HostKey 事件驱动确认弹窗：不重复报错、不在控制台记录指纹
    if (!isHostKeyError) {
      console.error('Connect failed:', e)
      // 非主机密钥失败：清理待确认参数，避免陈旧状态
      pendingConnectHost.value = null
      showToast(t('toast.connectionFailed', { err: String(e) }), 'error', 5000)
    }
  } finally { connecting.value = false }
}

// 密码弹框提交：携带密码执行直连
async function submitPasswordConnect() {
  const pc = pendingConnectHost.value
  if (!pc) return
  pendingConnectHost.value = null
  showPasswordPrompt.value = false
  await doConnectWith(pc.host, password.value)
}

function cancelConnect() {
  showPasswordPrompt.value = false
  password.value = ''
  connecting.value = false
  pendingConnectHost.value = null
}

// 主机密钥确认：Unknown（首次连接）/ Changed（密钥变更，可能 MITM）
async function handleHostKey(kind: string, detail: any) {
  const host = detail?.host
  const fingerprint = kind === 'Changed' ? detail?.new_fingerprint : detail?.fingerprint
  const oldFp = detail?.old_fingerprint
  if (!host || !fingerprint || !pendingConnectHost.value) return
  const pc = pendingConnectHost.value
  const msg = kind === 'Changed'
    ? t('hostkey.changed', { host, old: oldFp ?? '', new: fingerprint })
    : t('hostkey.unknown', { host, fp: fingerprint })
  const ok = await confirmDialog(msg, kind === 'Changed' ? t('hostkey.changedTitle') : t('hostkey.confirmTitle'))
  if (!ok) {
    // 拒绝信任：清理待确认参数与密码
    pendingConnectHost.value = null
    password.value = ''
    return
  }
  try {
    await invoke('approve_host_key', { host, port: pc.host.port, fingerprint })
    // 批准后自动重连
    pendingConnectHost.value = null
    await doConnectWith(pc.host, pc.password || null)
  } catch (e) {
    showToast(t('hostkey.saveFailed', { err: String(e) }), 'error', 5000)
    pendingConnectHost.value = null
  }
}

// 右键 Ping：调用 ping_host 命令，toast 显示结果
async function onPing(host: Host) {
  try {
    const r = await invoke('ping_host', { address: host.address }) as PingResult
    showToast(r.success
      ? (r.latency_ms != null ? t('toast.pingOk', { ms: String(r.latency_ms) }) : t('toast.pingOkNoLatency'))
      : t('toast.pingFail'), r.success ? 'success' : 'error', 5000)
  } catch (e) {
    showToast(t('toast.pingFail'), 'error', 5000)
  }
}

// 用户主目录（下载兜底目标）
const homeDir = ref('')

// 传输任务进度（流式传输事件更新，全局进度面板）
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
const downloading = ref<Record<string, boolean>>({})  // 下载防重入守卫
const uploading = ref<Record<string, boolean>>({})    // 上传防重入守卫

// 每标签刷新令牌：下载完成刷新对应标签的本地文件树，上传完成刷新远程文件树
const localRefresh = ref<Record<string, number>>({})
const remoteRefresh = ref<Record<string, number>>({})
function bumpLocalRefresh(sessionId: string) { localRefresh.value[sessionId] = (localRefresh.value[sessionId] ?? 0) + 1 }
function bumpRemoteRefresh(sessionId: string) { remoteRefresh.value[sessionId] = (remoteRefresh.value[sessionId] ?? 0) + 1 }

// SFTP 操作（按 tab 的 sessionId 调用）
// 下载目标优先级：拖拽目标目录 > 本地文件树当前目录（tab 内）> 用户主目录\Downloads > 用户主目录
// 注意：C:\Users 根目录因 UAC 权限限制不可写，切勿作为默认目标
async function downloadFile(sessionId: string, remotePath: string, localDir?: string) {
  if (!sessionId) return
  // 防重入：同一文件正在下载时忽略重复点击
  if (downloading.value[remotePath]) {
    showToast(t('toast.downloadInProgress'), 'info')
    return
  }
  downloading.value[remotePath] = true
  // 清洗文件名：替换 Windows 非法字符与路径分隔符，拒绝纯点（. 和 ..），防路径穿越
  const rawName = remotePath.split('/').pop() || 'download'
  const fileName = rawName.replace(/[\\/:*?"<>|]/g, '_').replace(/^\.+$/, '_')
  let dir = localDir || ''
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
      sessionId, remotePath, localPath, taskId, expectedDir: dir,
    })
    // 刷新对应标签的本地文件树
    bumpLocalRefresh(sessionId)
    showToast(t('toast.downloaded', { path: localPath }), 'success', 5000)
  } catch (e) {
    showToast(t('toast.downloadFailed', { err: String(e) }), 'error', 5000)
  } finally {
    downloading.value[remotePath] = false
    // 完成后短暂保留进度条（显示 100%），随后移除
    setTimeout(() => { delete transfers.value[taskId] }, 1500)
  }
}

async function uploadFile(sessionId: string, remoteDir: string, localPath: string, expectedDir?: string) {
  if (!sessionId) return
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
      sessionId, remotePath, localPath, taskId,
      expectedDir: expectedDir || homeDir.value || '',
    })
    // 刷新对应标签的远程文件树
    bumpRemoteRefresh(sessionId)
    showToast(t('toast.uploaded', { path: remotePath }), 'success', 5000)
  } catch (e) {
    showToast(t('toast.uploadFailed', { err: String(e) }), 'error', 5000)
  } finally {
    uploading.value[localPath] = false
    setTimeout(() => { delete transfers.value[taskId] }, 1500)
  }
}

// 对话框输入框（prompt 模式）
const dialogInput = ref('')
watch(() => dialogState.visible, (v) => { if (v) dialogInput.value = dialogState.defaultValue })

// 界面语言（i18n）
const locale = ref<Locale>(getLocale())
function onLocaleChange(l: Locale) {
  setLocale(l)
  locale.value = l
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
      // 传输锁定（Locked/Unlocked）按 tab 粒度接线由 Task B3 完成：
      // 本任务传输函数已按 tab 的 sessionId 调用，进度面板保持全局
      onTransfer: (_sid, _kind) => {},
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
        <!-- 无标签时显示占位提示 -->
        <div v-if="tabs.length === 0" class="placeholder"><p>{{ t('hosts.selectHint') }}</p></div>
        <!-- 标签：按 session_id 路由，v-show 切换保持各标签组件状态 -->
        <template v-for="tab in tabs" :key="tab.id">
          <SessionTab
            v-show="tab.id === activeTabId"
            :tab="tab"
            :local-refresh-key="localRefresh[tab.sessionId] ?? 0"
            :remote-refresh-key="remoteRefresh[tab.sessionId] ?? 0"
            @close="closeTab(tab.id)"
            @download="(p: string, dir?: string) => downloadFile(tab.sessionId, p, dir)"
            @upload="(dir: string, p: string, expectedDir?: string) => uploadFile(tab.sessionId, dir, p, expectedDir)"
          />
        </template>
      </div>
      <!-- 状态栏：左侧区域底部，展示运行状态（后续会话信息由 Task B3 完善） -->
      <div class="status-bar">
        <span class="status-badge">{{ status }}</span>
        <!-- 后续状态显示预留位 -->
      </div>
    </div>

    <!-- 右侧主机栏：双击直连 + 右键菜单 + 滑出表单面板 -->
    <HostPanel
      :hosts="hosts"
      :search-query="searchQuery"
      :locale="locale"
      @connect="connectHost"
      @edit="openEditPanel"
      @ping="onPing"
      @delete="onDeleteHost"
      @new="openNewPanel"
      @search="onSearch"
      @locale-change="onLocaleChange"
    />
    <HostFormPanel
      :open="panelOpen !== 'none'"
      :mode="panelOpen === 'none' ? 'new' : panelOpen"
      :initial="editingHost ? toForm(editingHost) : null"
      @save="saveHostForm"
      @cancel="cancelPanel"
    />

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

    <!-- 传输进度面板（全局） -->
    <TransferPanel :transfers="transfers" />

    <!-- 密码弹窗（双击直连密码认证） -->
    <div v-if="showPasswordPrompt" class="modal-overlay" @click.self="cancelConnect">
      <div class="modal">
        <h3>{{ t('hosts.enterPassword') }}</h3>
        <p>{{ t('hosts.connectingTo', { user: pendingConnectHost?.host.username || '', host: pendingConnectHost?.host.address || '' }) }}</p>
        <form @submit.prevent="submitPasswordConnect">
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
.status-badge { font-size: 0.7rem; color: hsla(160, 100%, 37%, 1); }

.placeholder { flex: 1; display: flex; align-items: center; justify-content: center; height: 100%; color: var(--color-text); opacity: 0.5; }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.btn-primary { background: hsla(160, 100%, 37%, 1); color: #fff; border-color: hsla(160, 100%, 37%, 1); }
.btn-primary:hover { background: hsla(160, 100%, 30%, 1); }

.modal-overlay { position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: var(--color-background); border: 1px solid var(--color-border); border-radius: 8px; padding: 1.5rem; min-width: 300px; box-shadow: 0 4px 24px rgba(0,0,0,0.3); }
.modal h3 { color: var(--color-heading); margin-bottom: 0.5rem; }
.modal p { color: var(--color-text); opacity: 0.7; font-size: 0.8rem; margin-bottom: 1rem; }
.modal input { width: 100%; padding: 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background-soft); color: var(--color-text); font-size: 0.85rem; margin-bottom: 0.8rem; box-sizing: border-box; }
.modal-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }

</style>
