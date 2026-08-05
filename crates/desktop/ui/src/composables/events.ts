// core-event 统一分派器：按 session_id 路由到对应 tab
// 解决跨会话事件污染：SessionEvent/ChannelEvent/TransferEvent 均携带 session_id，
// 前端据此更新对应标签页状态（数据事件仍由 Terminal 组件按 channel_id 自行过滤）

export interface TabEventRouter {
  onSession: (sessionId: string, kind: string, detail: any) => void
  onTransfer: (sessionId: string, kind: string, detail: any) => void
}

export function routeCoreEvent(payload: string, router: TabEventRouter) {
  let parsed: any
  try {
    parsed = JSON.parse(payload)
  } catch {
    return
  }
  const type = parsed.type
  const kind = parsed.payload?.kind
  const detail = parsed.payload?.detail ?? {}
  const sid: string | undefined = detail.session_id
  if (!sid) return
  if (type === 'Session' && router.onSession) router.onSession(sid, kind, detail)
  if (type === 'Transfer' && router.onTransfer) router.onTransfer(sid, kind, detail)
}
