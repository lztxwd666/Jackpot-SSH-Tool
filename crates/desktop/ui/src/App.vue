<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onBeforeUnmount, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import HostPanel, { type Host } from './components/HostPanel.vue'
import HostFormPanel, { type HostForm } from './components/HostFormPanel.vue'
import TransferPanel from './components/TransferPanel.vue'
import ToastStack from './components/ToastStack.vue'
import SessionTab, { type SessionTabState } from './components/SessionTab.vue'
import { routeCoreEvent } from './composables/events'
import { dispatchChannelData, dispatchChannelClosed } from './composables/channelData'
import { dialogState, closeDialog, confirmDialog, showToast } from './composables/dialog'
import { t, getLocale, setLocale, locales, localeNames, type Locale } from './composables/i18n'
import { type DragItem } from './composables/fs'
import { clamp } from './composables/pos'
import { startPanelDrag } from './composables/panelResize'
import { completeBoot } from './composables/boot'
import { useConnectFlow } from './composables/connect'
import { useTransfers } from './composables/transfer'

// 与后端 commands/host.rs 的 PingResult 结构对应
interface PingResult {
  success: boolean
  latency_ms: number | null
}

// 与后端 commands/sftp.rs 的 TransferProgress 结构对应
interface TransferProgress {
  id: string
  done: number
  total: number
  verifying?: boolean
  filename?: string
}

const hosts = ref<Host[]>([])
const searchQuery = ref('')
// 已有分组名列表（表单分组下拉选项，从主机数据聚合去重）
const allGroups = computed(() =>
  Array.from(new Set(hosts.value.map(h => h.group_name).filter((g): g is string => !!g))))
const status = ref('initializing')

// 布局宽度状态（VSCode 持久化布局惯例）：localStorage 初始化（读入即 clamp 越界值），
// 拖拽中实时更新、拖拽结束写入
const SIDEBAR_MIN = 180
const SIDEBAR_MAX = 600
const sidebarWidth = ref(clamp(Number(localStorage.getItem('layout.sidebarWidth')) || 220, SIDEBAR_MIN, SIDEBAR_MAX))
// 主机栏宽度提升为 CSS 变量：依赖其宽度的浮层（HostFormPanel 等）跟随联动
function applySidebarWidth() {
  document.documentElement.style.setProperty('--sidebar-width', `${sidebarWidth.value}px`)
}
function onSidebarDrag(e: PointerEvent) {
  startPanelDrag(e, (dx) => {
    sidebarWidth.value = clamp(sidebarWidth.value - dx, SIDEBAR_MIN, SIDEBAR_MAX)
    applySidebarWidth()
  }, () => {
    localStorage.setItem('layout.sidebarWidth', String(sidebarWidth.value))
  })
}
// 页面加载时初始化 CSS 变量与持久化宽度一致（否则 HostPanel 回退默认 220px）
onMounted(applySidebarWidth)

// 文件树宽度状态（与主机栏宽度同持久化惯例）：localStorage 初始化（读入即 clamp），
// 拖拽中实时更新（SessionTab 经 props 消费、emit 增量更新）、拖拽结束写入
const TREE_MIN = 120
const TREE_MAX = 480
const localWidth = ref(clamp(Number(localStorage.getItem('layout.localWidth')) || 180, TREE_MIN, TREE_MAX))
const remoteWidth = ref(clamp(Number(localStorage.getItem('layout.remoteWidth')) || 180, TREE_MIN, TREE_MAX))
function onResizeLocal(w: number) {
  localWidth.value = clamp(w, TREE_MIN, TREE_MAX)
}
function onResizeRemote(w: number) {
  remoteWidth.value = clamp(w, TREE_MIN, TREE_MAX)
}
function onResizeLocalEnd() {
  localStorage.setItem('layout.localWidth', String(localWidth.value))
}
function onResizeRemoteEnd() {
  localStorage.setItem('layout.remoteWidth', String(remoteWidth.value))
}

// 标签页工作区：多会话标签模型（旧单会话视图已在 Task 6 删除）
const tabs = ref<SessionTabState[]>([])
const activeTabId = ref<string | null>(null)

function openSessionTab(sessionId: string, hostId: string, hostName: string, address: string, channelId: string, status: SessionTabState['status'] = 'connected') {
  // 已存在同主机连接中的标签 → 聚焦（不重复建连）；按 hostId 匹配（name 非唯一，Task 9 由 hostName 迁移）
  const existing = tabs.value.find(t => t.hostId === hostId && t.status !== 'disconnected')
  if (existing) { activeTabId.value = existing.id; return existing.id }
  const tab: SessionTabState = { id: crypto.randomUUID(), hostId, hostName, address, sessionId, channelId, status, notices: [], cancelled: false }
  tabs.value.push(tab)
  activeTabId.value = tab.id
  return tab.id
}

function focusTab(tabId: string) {
  activeTabId.value = tabId
}

function closeTab(tabId: string) {
  const tab = tabs.value.find(t => t.id === tabId)
  if (!tab) return
  // 取消标记：进行中的重连流程在各检查点中止（迟到成功不得操作已关闭标签、不得新建通道）
  tab.cancelled = true
  // 取消该会话进行中的传输：进度条立即移除（后端 Close 命令同时中止 worker 传输），
  // 迟到结果静默——断开即取消，传输不得在会话关闭后继续跑
  cancelSessionTransfers(tab.sessionId)
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

// Host → 表单初始值（编辑时不回显密码，保留保存勾选状态）
function toForm(host: Host): HostForm {
  return {
    id: host.id, name: host.name, address: host.address, port: host.port,
    username: host.username, auth_type: host.auth_type, group_name: host.group_name,
    favorite: host.favorite, notes: host.notes, password: '',
    save_password: host.save_password,
  }
}

// 表单保存：save_host 更新主机配置；凭据部分按勾选/密码状态保存或删除
async function saveHostForm(form: HostForm) {
  try {
    const existing = form.id ? hosts.value.find(h => h.id === form.id) : undefined
    await invoke('save_host', {
      host: {
        id: form.id ?? crypto.randomUUID(),
        name: form.name, address: form.address, port: form.port,
        username: form.username, auth_type: form.auth_type, group_name: form.group_name,
        favorite: form.favorite, notes: form.notes,
        save_password: form.save_password,
        created_at: existing?.created_at ?? '',
        updated_at: new Date().toISOString(),
      },
    })
    if (form.save_password && form.password) {
      // 勾选保存且输入了密码：保存（或覆盖）OS 凭据库中的凭据
      const kind = form.auth_type === 'password' ? 'password' : 'passphrase'
      await invoke('save_credential', {
        host: form.address, port: form.port, username: form.username, kind,
        secret: form.password,
      })
    } else if (editingHost.value?.save_password && !form.save_password) {
      // 取消勾选：双 kind 清理（password + passphrase），覆盖切换 auth_type 后旧 kind 遗留的凭据
      // 命令幂等：未保存的 kind 返回成功，安全
      for (const kind of ['password', 'passphrase']) {
        await invoke('delete_credential', {
          host: form.address, port: form.port, username: form.username, kind,
        }).catch(() => {})
      }
    }
    cancelPanel()
    await loadHosts()
  } catch (e) { console.error('Save failed:', e) }
}

// 主机删除：确认后删除主机，连带关闭该主机的标签与已保存凭据
// 双 kind 清理（password + passphrase）：覆盖私钥主机曾存口令等任意遗留场景
async function onDeleteHost(host: Host) {
  const ok = await confirmDialog(t('hosts.deleteConfirm', { name: host.name }))
  if (!ok) return
  try {
    await invoke('delete_host', { id: host.id })
    // 连带清理已保存凭据（命令幂等：未保存的 kind 返回成功；失败不阻断删除流程）
    for (const kind of ['password', 'passphrase']) {
      await invoke('delete_credential', {
        host: host.address, port: host.port, username: host.username, kind,
      }).catch(() => {})
    }
    for (const tab of [...tabs.value]) {
      if (tab.hostId === host.id) closeTab(tab.id)
    }
    await loadHosts()
  } catch (e) { console.error('Delete failed:', e) }
}

// 主机 CRUD 与搜索
// 请求序号：loadHosts 与 doSearch 共用（写操作后刷新可能与搜索并发），过期响应
// 不得覆盖新结果（否则列表与搜索框内容错配，与文件树 loadSeq 同一竞态模式）
let listSeq = 0
let searchTimer: number | undefined
async function loadHosts() {
  const seq = ++listSeq
  try {
    const result = await invoke('list_hosts') as Host[]
    if (seq !== listSeq) return // 过期响应：已有更新的请求
    hosts.value = result
  } catch (e) { console.error('Failed to load hosts:', e) }
}
async function doSearch() {
  const seq = ++listSeq
  const q = searchQuery.value.trim()
  if (!q) { await loadHosts(); return }
  try {
    const result = await invoke('search_hosts', { query: q }) as Host[]
    if (seq !== listSeq) return // 过期响应：已有更新的请求
    hosts.value = result
  } catch (e) { console.error('Search failed:', e) }
}
function onSearch(q: string) {
  searchQuery.value = q
  // 防抖 150ms：连续输入不逐键发查询（search_hosts 为 SQL 全表 LIKE，无谓往返）
  if (searchTimer !== undefined) window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(doSearch, 150)
}

// 连接编排（双击直连/手动重连/主机密钥确认/密码弹框）与传输编排（下载/上传/进度面板）
const connect = useConnectFlow({ tabs, hosts, openTab: openSessionTab, focusTab })
const {
  connecting, password, showPasswordPrompt, promptHost, savePasswordOnConnect, passwordInputRef,
  connectHost, reconnectTab, handleHostKey, submitPromptPassword, cancelPromptPassword,
  upsertNotice, removeNotice, clearConnectionNotices, abandonedSessions,
} = connect

// 用户主目录（下载兜底目标；启动时经 transfer 消费）
const homeDir = ref('')
const transfer = useTransfers({ tabs, homeDir })
const {
  transfers, localRefresh, remoteRefresh,
  downloadFile, uploadFile, downloadMany, uploadMany, cancelSessionTransfers,
  handleTransferProgress,
} = transfer

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

// 切换收藏（星标点击）：save_host 持久化并同步本地列表
async function onToggleFavorite(host: Host) {
  const updated: Host = { ...host, favorite: !host.favorite }
  try {
    await invoke('save_host', {
      host: {
        id: updated.id, name: updated.name, address: updated.address, port: updated.port,
        username: updated.username, auth_type: updated.auth_type, group_name: updated.group_name,
        favorite: updated.favorite, notes: updated.notes, save_password: updated.save_password,
        created_at: updated.created_at, updated_at: new Date().toISOString(),
      },
    })
    const idx = hosts.value.findIndex(h => h.id === updated.id)
    if (idx >= 0) hosts.value[idx] = updated
  } catch (e) { console.error('Toggle favorite failed:', e) }
}

// 对话框输入框（prompt 模式）
const dialogInput = ref('')
const dialogInputRef = ref<HTMLInputElement>()
watch(() => dialogState.visible, (v) => {
  if (v) dialogInput.value = dialogState.defaultValue
  // 打开即聚焦（细节体验：用户无需再点一次输入框）
  if (v && dialogState.mode === 'prompt') {
    nextTick(() => dialogInputRef.value?.focus())
  }
})

// 界面语言（i18n）
const locale = ref<Locale>(getLocale())
function onLocaleChange(l: Locale) {
  setLocale(l)
  locale.value = l
}

let unlistenCore: () => void
let unlistenTransfer: () => void

onMounted(async () => {
  // 初始化数据并行拉取（运行状态/主机列表/主目录三个请求相互独立，串行等待无收益）
  await Promise.all([
    (async () => {
      // 查询真实运行时状态（不再硬编码 running）
      try {
        const appStatus = await invoke('get_app_status') as string
        status.value = appStatus === 'running' ? 'running' : 'stopped'
      } catch (_) { status.value = 'unknown' }
    })(),
    loadHosts(),
    (async () => {
      // 获取用户主目录（下载兜底目标）
      try { homeDir.value = await invoke('get_home_dir') } catch (_) {}
    })(),
  ])
  unlistenCore = await listen<any>('core-event', (event) => {
    // 后端直接 emit 事件对象（Tauri 序列化一次），payload 已是解析后对象，无需二次 parse
    const parsed = event.payload
    // 通道数据/关闭事件：分发给 Terminal 组件注册的写入器（应用级监听器启动即注册，
    // 无竞态——组件创建前的早期数据在 channelData 缓冲，注册时回放；
    // motd 等登录横幅在 shell 打开瞬间发出，早于前端渲染，此前因无消费者丢失）
    if (parsed.type === 'Channel') {
      const kind = parsed.payload?.kind
      const detail = parsed.payload?.detail
      if (kind === 'DataReceived' && detail?.channel_id) {
        // data 为 base64 编码的字节串（后端 serde_with::base64）
        const b64 = detail.data as string
        const bin = atob(b64)
        const bytes = new Uint8Array(bin.length)
        for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
        dispatchChannelData(detail.channel_id, bytes)
      } else if (kind === 'Closed' && detail?.channel_id) {
        dispatchChannelClosed(detail.channel_id)
      }
      return
    }
    // 非会话级事件（不带 session_id）：仍在本地处理
    if (parsed.type === 'Host') loadHosts()
    // 主机密钥确认流程：Unknown（首次）/ Changed（变更）
    if (parsed.type === 'HostKey' && (parsed.payload?.kind === 'Unknown' || parsed.payload?.kind === 'Changed')) {
      handleHostKey(parsed.payload.kind, parsed.payload?.detail)
    }
    // 会话级事件：按 session_id 路由到对应标签
    routeCoreEvent(parsed, {
      // onSession 完整状态机接线：Connecting / Connected / Disconnected（同时维护状态条 notices）
      onSession: (sid, kind, detail) => {
        const tab = tabs.value.find(t => t.sessionId === sid)
        if (!tab) return
        if (kind === 'Connecting') {
          tab.status = 'connecting'
          // 手动重连场景已显示"正在重连"提示：不叠加"正在连接"（语义重复）
          if (tab.notices.some(n => n.id === 'reconnecting')) return
          upsertNotice(tab, { id: 'connecting', level: 'info', message: t('tab.connecting') })
        } else if (kind === 'Connected') {
          // 已放弃的会话（流程超时取消）：迟到事件不得翻回状态（通道未重建，翻回会
          // 造成"连接中但终端冻结"的假象）；消费后移除标记（会话若重连成功重新生效）
          if (abandonedSessions.has(sid)) {
            abandonedSessions.delete(sid)
            return
          }
          tab.status = 'connected'
          tab.error = undefined
          clearConnectionNotices(tab)
        } else if (kind === 'Disconnected') {
          // 幂等：断开后跟 Close 可能重复广播 Disconnected，重复设置无副作用
          tab.status = 'disconnected'
          const reason = detail.reason ?? 'unknown'
          tab.error = reason
          // 状态条仅显示断开原因；重连按钮在断连遮罩中央（用户反馈：状态条不重复放按钮）
          upsertNotice(tab, {
            id: 'disconnected', level: 'error',
            message: t('tab.disconnected', { reason }),
          })
        }
      },
      // 传输锁（Locked/Unlocked）按 tab 粒度：锁定远程文件树 + 状态条提示（带宽占用，命令延后）
      onTransfer: (sid, kind) => {
        const tab = tabs.value.find(t => t.sessionId === sid)
        if (!tab) return
        tab.locked = kind === 'Locked'
        if (kind === 'Locked') {
          upsertNotice(tab, { id: 'transfer-busy', level: 'warning', message: t('tab.transferBusy') })
        } else {
          removeNotice(tab, 'transfer-busy')
        }
      },
    })
  })
  // 传输进度事件（流式下载/上传；与后端 commands/sftp.rs 的 TransferProgress 结构对应）
  // 进度更新与"校验中"toast 逻辑内聚在传输编排（transfer.ts）
  unlistenTransfer = await listen<TransferProgress>('transfer-progress', (event) => {
    handleTransferProgress(event.payload)
  })
  // 初始化完成（状态/主机列表/监听器全部就绪）：取消兜底并显示窗口，打开即完整 UI
  completeBoot()
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
            :locked="tab.locked"
            :local-width="localWidth"
            :remote-width="remoteWidth"
            @resize-local="onResizeLocal"
            @resize-remote="onResizeRemote"
            @resize-local-end="onResizeLocalEnd"
            @resize-remote-end="onResizeRemoteEnd"
            @close="closeTab(tab.id)"
            @reconnect="reconnectTab(tab)"
            @download="(p: string, dir?: string, isDir?: boolean) => downloadFile(tab.sessionId, p, dir, isDir)"
            @download-many="(items: DragItem[], dir?: string) => downloadMany(tab.sessionId, items, dir)"
            @upload="(dir: string, p: string, expectedDir?: string, isDir?: boolean) => uploadFile(tab.sessionId, dir, p, expectedDir, isDir)"
            @upload-many="(items: DragItem[], dir: string, expectedDir?: string) => uploadMany(tab.sessionId, dir, items, expectedDir)"
          />
        </template>
      </div>
      <!-- 状态栏：左侧区域底部，仅运行状态徽标（语言切换在右侧底部栏，用户反馈） -->
      <div class="status-bar">
        <span class="status-badge">{{ t('status.' + status) }}</span>
        <!-- 后续扩展：传输统计/网络状态等显示预留位 -->
      </div>
    </div>

    <!-- 面板分隔条：左区│主机栏 拖拽调整主机栏宽度（拖拽态与持久化见 onSidebarDrag） -->
    <div class="splitter" @pointerdown="onSidebarDrag" />
    <!-- 右侧区域：主机栏（上下贯通）+ 独立底部栏（语言切换 + 后续功能：设置图标等） -->
    <div class="right-area">
      <HostPanel
        :hosts="hosts"
        :search-query="searchQuery"
        @connect="connectHost"
        @edit="openEditPanel"
        @ping="onPing"
        @delete="onDeleteHost"
        @toggle-favorite="onToggleFavorite"
        @new="openNewPanel"
        @search="onSearch"
      />
      <!-- 右侧底部栏：与主机栏切割开的独立栏位（用户反馈：语言切换 + 后续设置图标等功能） -->
      <div class="right-footer">
        <!-- 语言选择器由 locales 数据驱动：新增语言只需在 i18n.ts 添加 locale 与 localeNames -->
        <select class="locale-select" :value="locale" @change="onLocaleChange(($event.target as HTMLSelectElement).value as Locale)">
          <option v-for="l in locales" :key="l" :value="l">{{ localeNames[l] }}</option>
        </select>
        <!-- 预留：设置图标等（后续） -->
      </div>
    </div>
    <HostFormPanel
      :open="panelOpen !== 'none'"
      :mode="panelOpen === 'none' ? 'new' : panelOpen"
      :initial="editingHost ? toForm(editingHost) : null"
      :groups="allGroups"
      @save="saveHostForm"
      @cancel="cancelPanel"
    />

    <!-- 全局确认/输入/多选项对话框（可反悔） -->
    <div v-if="dialogState.visible" class="modal-overlay" @click.self="closeDialog(dialogState.mode === 'prompt' ? null : false)">
      <div class="modal">
        <h3>{{ dialogState.title }}</h3>
        <p>{{ dialogState.message }}</p>
        <form v-if="dialogState.mode === 'prompt'" @submit.prevent="closeDialog(dialogInput)">
          <input ref="dialogInputRef" v-model="dialogInput" type="text" />
          <div class="modal-actions">
            <button type="submit" class="btn btn-primary">{{ t('common.ok') }}</button>
            <button type="button" class="btn" @click="closeDialog(null)">{{ t('common.cancel') }}</button>
          </div>
        </form>
        <!-- choice 模式：多选项按钮（首个为主操作，如重名冲突的"自动改名"） -->
        <div v-else-if="dialogState.mode === 'choice'" class="modal-actions">
          <button
            v-for="(c, i) in dialogState.choices"
            :key="c.value"
            class="btn"
            :class="{ 'btn-primary': i === 0 }"
            @click="closeDialog(c.value)"
          >{{ c.label }}</button>
          <button class="btn" @click="closeDialog(null)">{{ t('common.cancel') }}</button>
        </div>
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

    <!-- 密码弹窗（双击直连/手动重连密码认证，Promise 化） -->
    <div v-if="showPasswordPrompt" class="modal-overlay" @click.self="cancelPromptPassword">
      <div class="modal">
        <h3>{{ t('hosts.enterPassword') }}</h3>
        <p>{{ t('hosts.connectingTo', { user: promptHost?.username || '', host: promptHost?.address || '' }) }}</p>
        <form @submit.prevent="submitPromptPassword">
          <input ref="passwordInputRef" v-model="password" type="password" :placeholder="t('hosts.passwordPlaceholder')" required />
          <label class="modal-save-check">
            <input v-model="savePasswordOnConnect" type="checkbox" /> {{ t('hosts.savePasswordOnConnect') }}
          </label>
          <div class="modal-actions">
            <button type="submit" class="btn btn-primary" :disabled="connecting">{{ connecting ? t('common.connecting') : t('common.connect') }}</button>
            <button type="button" class="btn" @click="cancelPromptPassword">{{ t('common.cancel') }}</button>
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
/* 对齐不变量：3px 顶距 + 28px tab + 1px border-bottom = 32px（--bar-height 精确对齐，
   与 sidebar-header 底边同一水平线；改动 3px/28px 会静默破坏跨区域对齐） */
.tab-bar { display: flex; flex-wrap: wrap; /* 标签换行，不做横向滚动 */ background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); padding: 3px 4px 0; gap: 0.2rem; }
.tab { display: inline-flex; align-items: center; gap: 0.4rem; height: 28px; padding: 0 0.6rem; background: var(--color-background); border: 1px solid var(--color-border); border-radius: 4px 4px 0 0; cursor: pointer; font-size: 0.8rem; }
.tab.active { border-bottom-color: var(--color-background); background: var(--color-background-mute); }
.tab-dot { width: 8px; height: 8px; border-radius: 50%; }
.tab-dot.connected { background: var(--color-success); }
.tab-dot.connecting, .tab-dot.reconnecting { background: var(--color-warning); }
.tab-dot.disconnected { background: var(--color-danger); }
.tab-close { color: var(--color-text); opacity: 0.6; cursor: pointer; }
.tab-close:hover { opacity: 1; }
.tab-content { flex: 1; display: flex; overflow: hidden; }
.status-bar { height: var(--bar-height); padding: 0 0.6rem; border-top: 1px solid var(--color-border); font-size: 0.7rem; display: flex; align-items: center; }
.status-badge { font-size: 0.7rem; color: var(--color-success); }
/* 右侧区域：主机栏（flex:1 上下贯通）+ 底部独立栏（语言切换等）；与左区分隔线由 splitter 承担 */
.right-area { display: flex; flex-direction: column; }
/* 底部栏固定高度：滑出面板以其为 bottom 边界（不遮挡底部栏，用户反馈） */
.right-footer { height: var(--bar-height); box-sizing: border-box; padding: 0 0.6rem; border-top: 1px solid var(--color-border); display: flex; align-items: center; justify-content: flex-end; }
.locale-select { background: var(--color-background); color: var(--color-text); border: 1px solid var(--color-border); border-radius: 4px; font-size: 0.7rem; padding: 0.1rem 0.2rem; cursor: pointer; }

.placeholder { flex: 1; display: flex; align-items: center; justify-content: center; height: 100%; color: var(--color-text); opacity: 0.5; }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.btn-primary { background: var(--color-accent); color: #fff; border-color: var(--color-accent); }
.btn-primary:hover { background: color-mix(in srgb, var(--color-accent), black 12%); }

.modal-overlay { position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 100; }
.modal { background: var(--color-background); border: 1px solid var(--color-border); border-radius: 8px; padding: 1.5rem; min-width: 300px; box-shadow: 0 4px 24px rgba(0,0,0,0.3); }
.modal h3 { color: var(--color-heading); margin-bottom: 0.5rem; }
.modal p { color: var(--color-text); opacity: 0.7; font-size: 0.8rem; margin-bottom: 1rem; }
.modal input { width: 100%; padding: 0.4rem; border: 1px solid var(--color-border); border-radius: 4px; background: var(--color-background-soft); color: var(--color-text); font-size: 0.85rem; margin-bottom: 0.8rem; box-sizing: border-box; }
/* 密码弹框"保存此密码"勾选：checkbox 覆盖 modal input 的整行样式 */
.modal .modal-save-check { display: flex; align-items: center; gap: 0.4rem; font-size: 0.8rem; color: var(--color-text); margin-bottom: 0.8rem; }
.modal .modal-save-check input { width: auto; margin-bottom: 0; }
.modal-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }

</style>
