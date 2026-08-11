// 会话通道数据分发（应用级缓冲，解决早期数据丢失）。
// DataReceived/Closed 由 App.vue 全局监听器统一接收（应用启动即注册，无竞态窗口），
// 按 channel_id 分发给 Terminal 组件注册的写入器。Terminal 组件创建前到达的数据
// （motd 等登录横幅在 shell 打开瞬间发出，早于 open_shell IPC 返回与前端渲染）
// 缓冲于此，写入器注册时回放。此前由 Terminal 组件自行 listen，组件创建前的
// 数据无消费者被丢弃（表现为 stdout 正常但最早的登录横幅缺失）。

interface ChannelSink {
  onData: (bytes: Uint8Array) => void
  onClosed: () => void
}

/** 已注册的通道写入器（Terminal 组件存活期间注册） */
const sinks = new Map<string, ChannelSink>()
/** 组件创建前的早期数据缓冲（每通道），写入器注册时回放并清空 */
const pending = new Map<string, { data: Uint8Array[]; closed: boolean; bytes: number }>()
// 缓冲上限：早期数据只应是登录横幅等小体积内容（毫秒级渲染窗口）；
// 数据来自不可信服务器，防其在终端创建前灌入大流量拖垮内存（超出丢弃最旧，保住最新）
const PENDING_LIMIT = 64 * 1024

/** 注册通道写入器并回放缓冲的早期数据（组件创建时调用） */
export function registerChannelSink(channelId: string, sink: ChannelSink): void {
  sinks.set(channelId, sink)
  const buf = pending.get(channelId)
  if (buf) {
    for (const d of buf.data) sink.onData(d)
    if (buf.closed) sink.onClosed()
    pending.delete(channelId)
  }
}

/** 注销通道写入器并丢弃其缓冲（组件卸载后缓冲无意义） */
export function unregisterChannelSink(channelId: string): void {
  sinks.delete(channelId)
  pending.delete(channelId)
}

/** 分发通道数据（App.vue 全局监听器调用）；无写入器时缓冲 */
export function dispatchChannelData(channelId: string, bytes: Uint8Array): void {
  const sink = sinks.get(channelId)
  if (sink) {
    sink.onData(bytes)
  } else {
    let buf = pending.get(channelId)
    if (!buf) {
      buf = { data: [], closed: false, bytes: 0 }
      pending.set(channelId, buf)
    }
    buf.data.push(bytes)
    buf.bytes += bytes.length
    while (buf.bytes > PENDING_LIMIT && buf.data.length > 1) {
      buf.bytes -= buf.data[0].length
      buf.data.shift()
    }
  }
}

/** 分发通道关闭事件（App.vue 全局监听器调用）；无写入器时记录待回放 */
export function dispatchChannelClosed(channelId: string): void {
  const sink = sinks.get(channelId)
  if (sink) {
    sink.onClosed()
  } else {
    let buf = pending.get(channelId)
    if (!buf) {
      buf = { data: [], closed: false, bytes: 0 }
      pending.set(channelId, buf)
    }
    buf.closed = true
  }
}
