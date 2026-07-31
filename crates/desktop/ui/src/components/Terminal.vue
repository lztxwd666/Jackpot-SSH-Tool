<template>
  <div ref="terminalRef" class="terminal-container"></div>
</template>

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
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
  fitAddon.fit()
  term.focus()

  unlisten = await listen<string>('core-event', (event) => {
    try {
      const parsed = JSON.parse(event.payload)
      if (parsed.type === 'Channel' && parsed.payload.kind === 'DataReceived') {
        if (parsed.payload.detail.channel_id === props.channelId) {
          const data = parsed.payload.detail.data
          term.write(new Uint8Array(data))
        }
      }
    } catch (_) {}
  })

  term.onData((data) => {
    invoke('terminal_send_input', { channelId: props.channelId, data })
  })

  term.onResize(({ cols, rows }) => {
    invoke('terminal_resize', { channelId: props.channelId, cols, rows })
  })

  const observer = new ResizeObserver(() => {
    fitAddon.fit()
  })
  observer.observe(terminalRef.value!)
})

onBeforeUnmount(() => {
  unlisten?.()
  term?.dispose()
})
</script>

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
