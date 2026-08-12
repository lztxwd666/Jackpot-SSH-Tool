// 连接编排（双击直连 / 手动重连 / 主机密钥确认 / 密码弹框）
// 从 App.vue 拆分：连接状态机与标签/主机状态经 deps 注入解耦，对外仅暴露
// 模板绑定所需的状态与入口。并发约束：连接/重连互斥（connecting 标志），
// 同一时间最多一个密码弹框（promptResolve 单槽位）

import { ref, nextTick, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { confirmDialog, showToast } from './dialog'
import { t } from './i18n'
import type { SessionTabState, TabNotice } from '../components/SessionTab.vue'
import type { Host } from '../components/HostPanel.vue'

// 密码弹框的结果：null = 取消；非 null = 确认（secret 为密码，save 为"保存此密码"勾选）
type PasswordPromptResult = { secret: string; save: boolean } | null

// 连接/重连流程取消令牌：超时（onTimeout 置位）或标签关闭（tab.cancelled）时置位，
// 流程各 await 检查点中止，杜绝迟到成功产生的幽灵标签/状态回翻
interface FlowCancel { cancelled: boolean }

// 连接操作总超时（毫秒）：TCP 连接超时 15s + DNS 解析 + 认证/开通道余量。
// 防御性：即使后端挂起（worker 卡死/网络黑洞），前端 UI 不死锁——
// connecting 卡 true 会吞掉所有后续连接操作（用户实测"断开后无法连接"现象）
const CONNECT_TIMEOUT_MS = 30_000

/** 依赖注入：连接编排需要的外部状态与回调（App.vue 提供，避免 composable 持有全局单例） */
export interface ConnectFlowDeps {
  /** 标签列表（同主机去重、重连标签上下文查找） */
  tabs: Ref<SessionTabState[]>
  /** 主机列表（重连按 hostId 找回主机；保存凭据后同步 save_password 标志） */
  hosts: Ref<Host[]>
  /** 建标签回调（连接序列完成后打开标签；重连场景不复用） */
  openTab: (sessionId: string, hostId: string, hostName: string, address: string, channelId: string) => string
  /** 聚焦已有标签回调（双击已连接主机时聚焦而非重复建连） */
  focusTab: (tabId: string) => void
}

export function useConnectFlow(deps: ConnectFlowDeps) {
  const { tabs, hosts, openTab, focusTab } = deps

  const connecting = ref(false)
  const password = ref('')
  const showPasswordPrompt = ref(false)
  const promptHost = ref<Host | null>(null)
  // 密码弹框"保存此密码"勾选：确认时暂存，连接认证成功后落库凭据并更新 save_password 标志
  const savePasswordOnConnect = ref(false)
  const pendingSaveCredential = ref<{ host: Host; secret: string } | null>(null)
  // 密码弹框的 Promise resolver（同一时间最多一个挂起的弹框）
  let promptResolve: ((result: PasswordPromptResult) => void) | null = null
  // 待确认主机密钥时的连接参数（确认后自动重连/重连续跑）
  // reconnectTabId：手动重连场景携带标签上下文，密钥确认后更新现有标签而非新建
  const pendingConnectHost = ref<null | { host: Host; password: string; reconnectTabId?: string }>(null)
  const passwordInputRef = ref<HTMLInputElement>()

  // 已放弃的会话：流程取消（超时）后，后端迟到广播的 Connected 事件不得再操作 UI
  // （重连场景 connect_session 超时后仍可能成功，worker 广播 Connected 会把标签翻回
  // connected 但通道未重建，终端冻结遮罩消失）。手动重连开始时移除该会话的标记
  const abandonedSessions = new Set<string>()

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

  // 双击直连流程：保存过密码的主机静默加载凭据（未保存/加载失败则弹密码框）
  async function connectHost(host: Host) {
    // 防重入：连接进行中忽略新的双击
    if (connecting.value) return
    // 同主机已有活动标签：聚焦而非重复建连（按 hostId 匹配）
    const existing = tabs.value.find(t => t.hostId === host.id && t.status !== 'disconnected')
    if (existing) { focusTab(existing.id); return }
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
      openTab(sid, host.id, host.name, host.address, cid)
    } catch (e) {
      // 取消或失败统一回收会话；已关闭的会话再次 terminal_close 幂等无害
      // 仅取消路径登记放弃标记（防已广播的迟到 Connected 翻状态）；普通失败
      // （认证/TCP 失败）不会广播 Connected，登记只会产生只增不删的内存残留
      if (String(e).includes('connect-cancelled')) abandonedSessions.add(sid)
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
      // 打开即聚焦密码框（细节体验）
      nextTick(() => passwordInputRef.value?.focus())
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
  // 与 doConnectWith 的区别：不复用 openTab，直接更新 tab 的 channelId/status
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

  return {
    connecting, password, showPasswordPrompt, promptHost, savePasswordOnConnect, passwordInputRef,
    connectHost, reconnectTab, handleHostKey,
    submitPromptPassword, cancelPromptPassword,
    upsertNotice, removeNotice, clearConnectionNotices, restoreDisconnected,
    // 已放弃会话集合：App.vue 事件路由在 Connected 分支消费（has/delete 只读操作）
    abandonedSessions,
  }
}
