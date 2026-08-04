<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import 'xterm/css/xterm.css'

const props = defineProps<{ channelId: string }>()
const terminalRef = ref<HTMLDivElement>()
let term: Terminal
let fitAddon: FitAddon
let unlisten: () => void
let observer: ResizeObserver

onMounted(async () => {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    theme: {
      background: '#1e1e1e',
      foreground: '#d4d4d4',
    },
  })
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(terminalRef.value!)

  // onResize 必须在 fit() 之前注册，否则初始 resize 事件会丢失
  // resize 防抖：连续调整窗口时避免高频 IPC 调用
  let resizeTimer: number | undefined
  term.onResize(({ cols, rows }) => {
    if (resizeTimer !== undefined) window.clearTimeout(resizeTimer)
    resizeTimer = window.setTimeout(() => {
      invoke('terminal_resize', { channelId: props.channelId, cols, rows }).catch(() => {})
    }, 150)
  })

  fitAddon.fit()
  term.focus()

  // 第一次 fit 后手动发送 PTY 尺寸
  invoke('terminal_resize', {
    channelId: props.channelId,
    cols: term.cols,
    rows: term.rows,
  }).catch(() => {})

  unlisten = await listen<string>('core-event', (event) => {
    try {
      const parsed = JSON.parse(event.payload)
      if (parsed.type === 'Channel' && parsed.payload.kind === 'DataReceived') {
        if (parsed.payload.detail.channel_id === props.channelId) {
          // data 为 base64 编码的字节串（后端 serde_with::base64）
          const b64 = parsed.payload.detail.data as string
          const bin = atob(b64)
          const bytes = new Uint8Array(bin.length)
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
          term.write(bytes)
        }
      }
    } catch (_) {}
  })

  term.onData((data) => {
    invoke('terminal_send_input', { channelId: props.channelId, data }).catch(() => {})
  })

  observer = new ResizeObserver(() => {
    fitAddon.fit()
  })
  observer.observe(terminalRef.value!)

  // xterm 和事件监听器就绪后，通知后端开始读取 SSH 数据
  await invoke('start_terminal', { channelId: props.channelId })
})

onBeforeUnmount(() => {
  observer?.disconnect()
  unlisten?.()
  term?.dispose()
})
</script>

<template>
  <div ref="terminalRef" class="terminal-container"></div>
</template>

<style scoped>
.terminal-container {
  width: 100%;
  height: 100%;
}
.terminal-container :deep(.xterm) {
  height: 100%;
}
.terminal-container :deep(.xterm-viewport) {
  overflow-y: auto;
}
</style>
