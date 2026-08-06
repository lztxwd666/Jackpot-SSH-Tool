<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import HostPanel, { type Host } from './components/HostPanel.vue'
import HostFormPanel, { type HostForm } from './components/HostFormPanel.vue'
import TransferPanel from './components/TransferPanel.vue'
import ToastStack from './components/ToastStack.vue'
import SessionTab, { type SessionTabState, type TabNotice } from './components/SessionTab.vue'
import { routeCoreEvent } from './composables/events'
import { dialogState, closeDialog, confirmDialog, showToast } from './composables/dialog'
import { t, getLocale, setLocale, locales, localeNames, type Locale } from './composables/i18n'

// 与后端 commands/host.rs 的 PingResult 结构对应
interface PingResult {
  success: boolean
  latency_ms: number | null
}

const hosts = ref<Host[]>([])
const searchQuery = ref('')
// 已有分组名列表（表单分组下拉选项，从主机数据聚合去重）
const allGroups = computed(() =>
  Array.from(new Set(hosts.value.map(h => h.group_name).filter((g): g is string => !!g))))
const status = ref('initializing')

// 双击直连/手动重连状态：密码弹框（Promise 化，确认 resolve 密码+保存勾选 / 取消 resolve null）
const connecting = ref(false)
const password = ref('')
const showPasswordPrompt = ref(false)
const promptHost = ref<Host | null>(null)
// 密码弹框"保存此密码"勾选：确认时暂存，连接认证成功后落库凭据并更新 save_password 标志
const savePasswordOnConnect = ref(false)
const pendingSaveCredential = ref<{ host: Host; secret: string } | null>(null)
// 密码弹框的结果：null = 取消；非 null = 确认（secret 为密码，save 为"保存此密码"勾选）
type PasswordPromptResult = { secret: string; save: boolean } | null
// 密码弹框的 Promise resolver（同一时间最多一个挂起的弹框）
let promptResolve: ((result: PasswordPromptResult) => void) | null = null
// 待确认主机密钥时的连接参数（确认后自动重连/重连续跑）
// reconnectTabId：手动重连场景携带标签上下文，密钥确认后更新现有标签而非新建
const pendingConnectHost = ref<null | { host: Host; password: string; reconnectTabId?: string }>(null)

// 标签页工作区：多会话标签模型（旧单会话视图已在 Task 6 删除）
const tabs = ref<SessionTabState[]>([])
const activeTabId = ref<string | null>(null)

function activeTab() {
  return tabs.value.find(t => t.id === activeTabId.value) ?? null
}

function openSessionTab(sessionId: string, hostId: string, hostName: string, address: string, channelId: string, status: SessionTabState['status'] = 'connected') {
  // 已存在同主机连接中的标签 → 聚焦（不重复建连）；按 hostId 匹配（name 非唯一，Task 9 由 hostName 迁移）
  const existing = tabs.value.find(t => t.hostId === hostId && t.status !== 'disconnected')
  if (existing) { activeTabId.value = existing.id; return existing.id }
  const tab: SessionTabState = { id: crypto.randomUUID(), hostId, hostName, address, sessionId, channelId, status, notices: [], cancelled: false }
  tabs.value.push(tab)
  activeTabId.value = tab.id
  return tab.id
}

// 状态条提示维护（tab.notices upsert/remove，事件驱动；新提示 = 分派器加一条映射，渲染零改动）
function upsertNotice(tab: SessionTabState, notice: TabNotice) {
  const i = tab.notices.findIndex(n => n.id === notice.id)
  if (i >= 0) tab.notices[i] = notice
  else tab.notices.push(notice)
}
function removeNotice(tab: SessionTabState, id: string) {
  const i = tab.notices.findIndex(n => n.id === id)
  if (i >= 0) tab.notices.splice(i, 1)
}
// 连接状态类提示整体清除（连接成功时：connecting/disconnected/reconnecting 都不再适用）
function clearConnectionNotices(tab: SessionTabState) {
  tab.notices = tab.notices.filter(n => !['connecting', 'disconnected', 'reconnecting'].includes(n.id))
}
// 恢复断连提示（重连被取消/拒绝/失败时：从 reconnecting 回到 disconnected 状态条；
// 要求调用方先设置 tab.error，原断开原因或失败原因）
function restoreDisconnected(tab: SessionTabState) {
  tab.status = 'disconnected'
  removeNotice(tab, 'reconnecting')
  upsertNotice(tab, {
    id: 'disconnected', level: 'error',
    message: t('tab.disconnected', { reason: tab.error || '' }),
  })
}

function closeTab(tabId: string) {
  const tab = tabs.value.find(t => t.id === tabId)
  if (!tab) return
  // 取消标记：进行中的重连流程在各检查点中止（迟到成功不得操作已关闭标签、不得新建通道）
  tab.cancelled = true
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

// 双击直连流程：保存过密码的主机静默加载凭据（未保存/加载失败则弹密码框）
async function connectHost(host: Host) {
  // 防重入：连接进行中忽略新的双击
  if (connecting.value) return
  // 同主机已有活动标签：聚焦而非重复建连（按 hostId 匹配）
  const existing = tabs.value.find(t => t.hostId === host.id && t.status !== 'disconnected')
  if (existing) { activeTabId.value = existing.id; return }
  let secret: string | null = null
  if (host.auth_type === 'password') {
    // 已保存密码的主机：静默读取凭据；读取失败降级为弹框
    if (host.save_password) {
      secret = await invoke('load_credential', {
        host: host.address, port: host.port, username: host.username, kind: 'password',
      }).catch(() => null) as string | null
    }
    if (secret == null) {
      // 弹密码框：确认拿到密码后直连，取消则不动作
      const r = await promptPassword(host)
      if (r == null) return
      secret = r.secret
      // 勾选"保存此密码"：暂存凭据，连接认证成功后落库
      if (r.save) pendingSaveCredential.value = { host, secret: r.secret }
    }
  }
  await doConnectWith(host, secret)
}

// 连接操作总超时（毫秒）：TCP 连接超时 30s + 认证/开通道余量。
// 防御性：即使后端挂起（worker 卡死/网络黑洞），前端 UI 不死锁——
// connecting 卡 true 会吞掉所有后续连接操作（用户实测"断开后无法连接"现象）
const CONNECT_TIMEOUT_MS = 45_000

// 连接/重连流程取消令牌：超时（onTimeout 置位）或标签关闭（tab.cancelled）时置位，
// 流程各 await 检查点中止，杜绝迟到成功产生的幽灵标签/状态回翻
interface FlowCancel { cancelled: boolean }

// 已放弃的会话：流程取消（超时）后，后端迟到广播的 Connected 事件不得再操作 UI
// （重连场景 connect_session 超时后仍可能成功，worker 广播 Connected 会把标签翻回
// connected 但通道未重建，终端冻结遮罩消失）。手动重连开始时移除该会话的标记
const abandonedSessions = new Set<string>()

// 带超时的 Promise 包装：超时 reject 标记错误 'connect-timeout'，并回调 onTimeout
// （调用方借此置位取消令牌）；超时后原 Promise 迟到 settle 被 then 消费，无 unhandled rejection
function withTimeout<T>(p: Promise<T>, ms: number, onTimeout?: () => void): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => { onTimeout?.(); reject(new Error('connect-timeout')) }, ms)
    p.then(
      (v) => { window.clearTimeout(timer); resolve(v) },
      (e) => { window.clearTimeout(timer); reject(e) },
    )
  })
}

// 执行连接：create_session → connect_session → open_shell → 打开标签
async function doConnectWith(host: Host, secret: string | null) {
  connecting.value = true
  const cancel: FlowCancel = { cancelled: false }
  try {
    await withTimeout(connectFlow(host, secret, cancel), CONNECT_TIMEOUT_MS, () => { cancel.cancelled = true })
  } catch (e) {
    handleConnectError(e)
  } finally { connecting.value = false }
}

// 连接序列（超时保护范围：create_session 起至标签打开）
// 取消（超时）路径：各 await 检查点抛 'connect-cancelled'，由 handleConnectError 静默清理；
// 统一在 catch 回收已创建/已连接的 session（取消与失败路径均不残留后端会话）
async function discardSession(sessionId: string) {
  await invoke('terminal_close', { sessionId }).catch(() => {})
}
async function connectFlow(host: Host, secret: string | null, cancel: FlowCancel) {
  const sid = await invoke('create_session') as string
  try {
    if (cancel.cancelled) throw new Error('connect-cancelled')
    // 保存连接参数：主机密钥确认后自动重连需要
    pendingConnectHost.value = { host, password: secret ?? '' }
    await invoke('connect_session', {
      sessionId: sid, host: host.address, port: host.port,
      username: host.username, authType: host.auth_type,
      password: secret, privateKeyPath: null, privateKeyPassphrase: null,
    })
    if (cancel.cancelled) throw new Error('connect-cancelled')
    // 连接成功后及时清空密码（减少在 JS 堆中的驻留时间）
    password.value = ''
    pendingConnectHost.value = null
    // 认证已通过：弹框勾选"保存此密码"的凭据在此落库
    await applyPendingCredential()
    const cid = await invoke('open_shell', { sessionId: sid }) as string
    if (cancel.cancelled) throw new Error('connect-cancelled')
    openSessionTab(sid, host.id, host.name, host.address, cid)
  } catch (e) {
    // 取消或失败统一回收会话；已关闭的会话再次 terminal_close 幂等无害
    abandonedSessions.add(sid)
    await discardSession(sid)
    throw e
  }
}

// 连接错误统一处理：取消 / 超时 / 主机密钥（事件驱动，不处理） / 普通错误
function handleConnectError(e: unknown) {
  const msg = String(e)
  // 流程取消（超时已弹过 toast，或标签关闭）：静默清理待确认参数与待保存凭据
  if (msg.includes('connect-cancelled')) {
    pendingConnectHost.value = null
    pendingSaveCredential.value = null
    return
  }
  // 主机密钥场景由 HostKey 事件驱动确认弹窗：不重复报错、不在控制台记录指纹
  if (msg.includes('host key')) return
  // 超时：清理待确认参数与待保存凭据（连接未建立，凭据不落库）；未完成的 session 由 Drop 回收
  if (msg.includes('connect-timeout')) {
    console.error('Connect timeout:', e)
    pendingConnectHost.value = null
    pendingSaveCredential.value = null
    showToast(t('toast.connectTimeout'), 'error', 5000)
    return
  }
  console.error('Connect failed:', e)
  // 非主机密钥失败：清理待确认参数与待保存凭据，避免陈旧状态（认证未通过，密码无效不保存）
  pendingConnectHost.value = null
  pendingSaveCredential.value = null
  showToast(t('toast.connectionFailed', { err: msg }), 'error', 5000)
}

// 密码弹窗（Promise 化）：确认 → resolve { secret, save }；取消 → resolve null
function promptPassword(host: Host): Promise<PasswordPromptResult> {
  return new Promise((resolve) => {
    // 并发守卫：已有密码弹窗时立即取消（避免旧 Promise 永久挂起）
    if (promptResolve) { resolve(null); return }
    promptResolve = resolve
    promptHost.value = host
    password.value = ''
    savePasswordOnConnect.value = false
    showPasswordPrompt.value = true
  })
}

// 密码弹框确认：resolve 密码与保存勾选并关闭
function submitPromptPassword() {
  const r = promptResolve
  if (!r) return
  promptResolve = null
  showPasswordPrompt.value = false
  r({ secret: password.value, save: savePasswordOnConnect.value })
  password.value = ''
}

// 密码弹框取消：resolve null（调用方不发起连接）
function cancelPromptPassword() {
  const r = promptResolve
  if (!r) return
  promptResolve = null
  showPasswordPrompt.value = false
  r(null)
  password.value = ''
}

// 弹框勾选"保存此密码"且连接认证成功后落库：
// 先 save_credential 写入 OS 凭据库，再 save_host 更新 save_password 标志（下次连接走静默加载）
// 只有认证通过（connect_session 成功）才消费；save_credential 失败则不落库；
// save_host 失败时凭据已落库但标志未更新（下次连接仍走弹框，行为可回退）
async function applyPendingCredential() {
  const pc = pendingSaveCredential.value
  if (!pc) return
  pendingSaveCredential.value = null
  try {
    await invoke('save_credential', {
      host: pc.host.address, port: pc.host.port, username: pc.host.username,
      kind: 'password', secret: pc.secret,
    })
    const updated: Host = { ...pc.host, save_password: true }
    await invoke('save_host', {
      host: {
        id: updated.id, name: updated.name, address: updated.address, port: updated.port,
        username: updated.username, auth_type: updated.auth_type, group_name: updated.group_name,
        favorite: updated.favorite, notes: updated.notes, save_password: true,
        created_at: updated.created_at, updated_at: new Date().toISOString(),
      },
    })
    // 同步本地列表（编辑面板的勾选状态与双击直连的静默加载都依赖此字段）
    const idx = hosts.value.findIndex(h => h.id === updated.id)
    if (idx >= 0) hosts.value[idx] = updated
  } catch (e) { console.error('Save credential failed:', e) }
}

// 手动重连流程（用户主动操作；从不自动触发重连）
// 复用同一 SessionId 重新 connect_session → open_shell，新通道 ID 触发 Terminal :key 重建
async function reconnectTab(tab: SessionTabState) {
  if (!tab.sessionId) return
  // 互斥：连接/重连进行中忽略新的重连请求（与 connectHost 一致，
  // 防并发流程共享 pendingConnectHost/pendingSaveCredential 导致上下文错配）
  if (connecting.value) return
  // 手动重连开始：会话恢复活跃（清除之前的放弃标记，迟到事件重新生效）
  abandonedSessions.delete(tab.sessionId)
  tab.status = 'reconnecting'
  // 按 hostId 找回主机（重命名后 hostName 已过期，hostId 才是稳定标识）
  const host = hosts.value.find(h => h.id === tab.hostId)
  if (!host) {
    // 主机已删除（正常删除流程会连带关闭标签，此处为防御兜底）
    tab.error = t('toast.hostNotFound')
    restoreDisconnected(tab)
    return
  }
  let secret: string | null = null
  if (host.auth_type === 'password') {
    // 已保存密码的主机：静默读取凭据；读取失败降级为弹框
    if (host.save_password) {
      secret = await invoke('load_credential', {
        host: host.address, port: host.port, username: host.username, kind: 'password',
      }).catch(() => null) as string | null
    }
    if (secret == null) {
      const r = await promptPassword(host)
      if (r == null) {
        // 用户取消：不发连接请求，恢复断连状态（原断开原因保留）
        restoreDisconnected(tab)
        return
      }
      secret = r.secret
      // 勾选"保存此密码"：暂存凭据，连接认证成功后落库
      if (r.save) pendingSaveCredential.value = { host, secret: r.secret }
    }
  }
  // 手动重连进行中：状态条提示（先移除断开提示，避免并存；成功由 Connected 事件清除，失败由 catch 恢复）
  tab.status = 'reconnecting'
  removeNotice(tab, 'disconnected')
  upsertNotice(tab, { id: 'reconnecting', level: 'info', message: t('tab.reconnecting') })
  // 保存连接参数：主机密钥确认后自动重连需要（携带重连上下文，确认后更新现有标签）
  pendingConnectHost.value = { host, password: secret ?? '', reconnectTabId: tab.id }
  await doReconnectWith(host, secret, tab)
}

// 执行重连连接序列：connect_session → open_shell → 更新现有标签（不新建标签）
// 与 doConnectWith 的区别：不复用 openSessionTab，直接更新 tab 的 channelId/status
async function doReconnectWith(host: Host, secret: string | null, tab: SessionTabState) {
  connecting.value = true
  const cancel: FlowCancel = { cancelled: false }
  try {
    await withTimeout(reconnectFlow(host, secret, tab, cancel), CONNECT_TIMEOUT_MS, () => { cancel.cancelled = true })
  } catch (e) {
    const msg = String(e)
    // 流程取消（超时已弹过 toast / 标签已关闭）：静默，不重复报错
    if (msg.includes('connect-cancelled') || tab.cancelled) {
      pendingConnectHost.value = null
      pendingSaveCredential.value = null
      return
    }
    const isHostKeyError = msg.includes('host key')
    // 主机密钥场景由 HostKey 事件驱动确认弹窗：保留 pendingConnectHost 与断开原因，
    // 不重复报错、不改状态（确认/拒绝后的终态由 handleHostKey 决定）
    if (!isHostKeyError) {
      pendingConnectHost.value = null
      // 认证未通过：清理待保存凭据（密码无效不落库）
      pendingSaveCredential.value = null
      tab.error = msg.includes('connect-timeout') ? t('toast.connectTimeout') : msg
      // 重连失败：状态条恢复断开提示（含原因；重连按钮在断连遮罩中央）
      restoreDisconnected(tab)
      showToast(t('toast.connectionFailed', { err: msg }), 'error', 5000)
    }
  } finally {
    connecting.value = false
  }
}

// 重连序列（超时保护范围同 doConnectWith）
// 取消检查：cancel（超时）或 tab.cancelled（标签关闭）；会话复用不回收（用户可再次重连）
async function reconnectFlow(host: Host, secret: string | null, tab: SessionTabState, cancel: FlowCancel) {
  await invoke('connect_session', {
    sessionId: tab.sessionId, host: host.address, port: host.port,
    username: host.username, authType: host.auth_type,
    password: secret, privateKeyPath: null, privateKeyPassphrase: null,
  })
  if (cancel.cancelled || tab.cancelled) {
    // 会话保持连接（复用不回收，用户可再次重连）；放弃标记过滤其迟到 Connected 事件
    abandonedSessions.add(tab.sessionId)
    throw new Error('connect-cancelled')
  }
  // 连接成功后及时清空密码（减少在 JS 堆中的驻留时间）
  password.value = ''
  pendingConnectHost.value = null
  // 认证已通过：弹框勾选"保存此密码"的凭据在此落库
  await applyPendingCredential()
  const cid = await invoke('open_shell', { sessionId: tab.sessionId }) as string
  if (cancel.cancelled || tab.cancelled) {
    abandonedSessions.add(tab.sessionId)
    throw new Error('connect-cancelled')
  }
  // 新通道 ID 触发 Terminal :key 重建：旧终端画面作废，全新会话视图
  tab.channelId = cid
  tab.status = 'connected'
  tab.error = undefined
}

// 主机密钥确认：Unknown（首次连接）/ Changed（密钥变更，可能 MITM）
// 手动重连场景（pendingConnectHost.reconnectTabId）：确认后继续重连流程（更新现有标签），拒绝则恢复断连状态
async function handleHostKey(kind: string, detail: any) {
  const host = detail?.host
  // key_type 随事件透传：approve 时按真实类型存储（known_hosts 按 (host, port, key_type) 匹配）
  const keyType = detail?.key_type ?? ''
  const fingerprint = kind === 'Changed' ? detail?.new_fingerprint : detail?.fingerprint
  const oldFp = detail?.old_fingerprint
  if (!host || !fingerprint || !pendingConnectHost.value) return
  const pc = pendingConnectHost.value
  // 重连上下文的目标标签（标签可能已被用户关闭，允许为空则退回首次连接语义）
  const targetTab = pc.reconnectTabId
    ? tabs.value.find(t => t.id === pc.reconnectTabId) ?? null
    : null
  const msg = kind === 'Changed'
    ? t('hostkey.changed', { host, old: oldFp ?? '', new: fingerprint })
    : t('hostkey.unknown', { host, fp: fingerprint })
  const ok = await confirmDialog(msg, kind === 'Changed' ? t('hostkey.changedTitle') : t('hostkey.confirmTitle'))
  if (!ok) {
    // 拒绝信任：清理待确认参数、密码与待保存凭据（拒绝即用户取消，凭据从未认证通过）
    // 重连场景恢复断连状态（原断开原因保留）
    pendingConnectHost.value = null
    pendingSaveCredential.value = null
    password.value = ''
    if (targetTab) restoreDisconnected(targetTab)
    return
  }
  try {
    await invoke('approve_host_key', { host, port: pc.host.port, keyType, fingerprint })
    // 批准后自动重连：重连场景继续 doReconnectWith（更新现有标签），首次连接走 doConnectWith
    // 待保存凭据仅保留在此路径：批准成功后的重试中认证通过才会消费
    pendingConnectHost.value = null
    if (targetTab) {
      await doReconnectWith(pc.host, pc.password || null, targetTab)
      return
    }
    await doConnectWith(pc.host, pc.password || null)
  } catch (e) {
    showToast(t('hostkey.saveFailed', { err: String(e) }), 'error', 5000)
    pendingConnectHost.value = null
    pendingSaveCredential.value = null
    // 密钥保存失败：重连场景同样恢复断连状态（避免卡在 reconnecting）
    if (targetTab) restoreDisconnected(targetTab)
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
  unlistenCore = await listen<any>('core-event', (event) => {
    // 后端直接 emit 事件对象（Tauri 序列化一次），payload 已是解析后对象，无需二次 parse
    const parsed = event.payload
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
            :locked="tab.locked"
            @close="closeTab(tab.id)"
            @reconnect="reconnectTab(tab)"
            @download="(p: string, dir?: string) => downloadFile(tab.sessionId, p, dir)"
            @upload="(dir: string, p: string, expectedDir?: string) => uploadFile(tab.sessionId, dir, p, expectedDir)"
          />
        </template>
      </div>
      <!-- 状态栏：左侧区域底部，仅运行状态徽标（语言切换在右侧底部栏，用户反馈） -->
      <div class="status-bar">
        <span class="status-badge">{{ t('status.' + status) }}</span>
        <!-- 后续扩展：传输统计/网络状态等显示预留位 -->
      </div>
    </div>

    <!-- 右侧区域：主机栏（上下贯通）+ 独立底部栏（语言切换 + 后续功能：设置图标等） -->
    <div class="right-area">
      <HostPanel
        :hosts="hosts"
        :search-query="searchQuery"
        @connect="connectHost"
        @edit="openEditPanel"
        @ping="onPing"
        @delete="onDeleteHost"
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

    <!-- 密码弹窗（双击直连/手动重连密码认证，Promise 化） -->
    <div v-if="showPasswordPrompt" class="modal-overlay" @click.self="cancelPromptPassword">
      <div class="modal">
        <h3>{{ t('hosts.enterPassword') }}</h3>
        <p>{{ t('hosts.connectingTo', { user: promptHost?.username || '', host: promptHost?.address || '' }) }}</p>
        <form @submit.prevent="submitPromptPassword">
          <input v-model="password" type="password" :placeholder="t('hosts.passwordPlaceholder')" autofocus required />
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
.status-bar { padding: 0.3rem 0.6rem; border-top: 1px solid var(--color-border); font-size: 0.7rem; display: flex; align-items: center; }
.status-badge { font-size: 0.7rem; color: hsla(160, 100%, 37%, 1); }
/* 右侧区域：主机栏（flex:1 上下贯通）+ 底部独立栏（语言切换等）；border-left 分隔左右区域 */
.right-area { display: flex; flex-direction: column; border-left: 1px solid var(--color-border); }
/* 底部栏固定高度：滑出面板以其为 bottom 边界（不遮挡底部栏，用户反馈） */
.right-footer { height: 32px; box-sizing: border-box; padding: 0 0.6rem; border-top: 1px solid var(--color-border); display: flex; align-items: center; justify-content: flex-end; }
.locale-select { background: var(--color-background); color: var(--color-text); border: 1px solid var(--color-border); border-radius: 4px; font-size: 0.7rem; padding: 0.1rem 0.2rem; cursor: pointer; }

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
/* 密码弹框"保存此密码"勾选：checkbox 覆盖 modal input 的整行样式 */
.modal .modal-save-check { display: flex; align-items: center; gap: 0.4rem; font-size: 0.8rem; color: var(--color-text); margin-bottom: 0.8rem; }
.modal .modal-save-check input { width: auto; margin-bottom: 0; }
.modal-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }

</style>
