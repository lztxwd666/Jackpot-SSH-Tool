<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { showToast } from '../composables/dialog'
import { clampFloatPos } from '../composables/pos'
import { useClickOutsideClose } from '../composables/menu'
import { t } from '../composables/i18n'
import 'xterm/css/xterm.css'

const props = defineProps<{ channelId: string }>()
const terminalRef = ref<HTMLDivElement>()
let term: Terminal
let fitAddon: FitAddon
let unlisten: () => void
let observer: ResizeObserver
let resizeTimer: number | undefined
// 卸载标记：await listen 期间组件可能被卸载（重连 :key 重建），
// 返回后检查标记解绑监听器并中止初始化，防监听器泄漏与对已 dispose 终端的写入
let disposed = false
// 会话结束标记：shell EOF 远端关闭通道（Closed 事件）后停止发送输入并显示提示
// （否则输入继续走 IPC 报 channel not found 刷日志，终端表现为卡死）
let ended = false

// 终端右键菜单（MobaXterm 风格：复制/粘贴/清屏）
// 复制项在无选中文本时禁用（xterm selection 状态跟踪）
const termMenu = ref<{ x: number; y: number } | null>(null)
const hasSelection = ref(false)
const termMenuStyle = computed(() => {
  if (!termMenu.value) return { left: '0px', top: '0px' }
  const p = clampFloatPos(termMenu.value.x, termMenu.value.y, 140, 100)
  return { left: p.x + 'px', top: p.y + 'px' }
})
function onTermContextMenu(e: MouseEvent) {
  // 阻止浏览器默认菜单（xterm 内部隐藏 textarea 会被全局抑制放行，必须在此拦截）
  e.preventDefault()
  termMenu.value = { x: e.clientX, y: e.clientY }
}
function closeTermMenu() { termMenu.value = null }
useClickOutsideClose(termMenu, closeTermMenu)

// 复制选中文本（xterm 剪贴板惯例：复制成功无提示，失败提示）
async function doCopy() {
  if (!hasSelection.value) return
  closeTermMenu()
  try {
    await navigator.clipboard.writeText(term.getSelection())
  } catch {
    showToast(t('term.copyFailed'), 'error')
  }
}

// 粘贴（xterm paste 模拟键盘输入，经 onData 发送到远端）
async function doPaste() {
  closeTermMenu()
  try {
    const text = await navigator.clipboard.readText()
    term.paste(text)
  } catch {
    showToast(t('term.pasteFailed'), 'error')
  }
}

// 清屏（本地清除终端显示，不动远端会话）
function doClear() {
  closeTermMenu()
  term.clear()
}

onMounted(async () => {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    // 终端配色独立于界面主题（OneDark ANSI 色板，实测自 Binaryify/OneDark-Pro，MIT）
    // 背景固定经典黑底 #1e1e1e，不随界面主题切换；其余色值为仓库实测值
    theme: {
      background: '#1e1e1e', // 经典黑底固定，不随主题切换
      foreground: '#abb2bf',
      cursor: '#61afef', // 仓库未定义 terminalCursor 键，采用设计值（OneDark 蓝）
      cursorAccent: '#1e1e1e',
      selectionBackground: '#abb2bf30',
      black: '#3f4451',
      red: '#e05561',
      green: '#8cc265',
      yellow: '#d18f52',
      blue: '#4aa5f0',
      magenta: '#c162de',
      cyan: '#42b3c2',
      white: '#d7dae0',
      brightBlack: '#4f5666',
      brightRed: '#ff616e',
      brightGreen: '#a5e075',
      brightYellow: '#f0a45d',
      brightBlue: '#4dc4ff',
      brightMagenta: '#de73ff',
      brightCyan: '#4cd1e0',
      brightWhite: '#e6e6e6',
    },
  })
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(terminalRef.value!)

  // 选中状态跟踪（右键菜单"复制"项禁用态）
  term.onSelectionChange(() => {
    hasSelection.value = term.hasSelection()
  })

  // onResize 必须在 fit() 之前注册，否则初始 resize 事件会丢失
  // resize 防抖：连续调整窗口时避免高频 IPC 调用
  term.onResize(({ cols, rows }) => {
    if (resizeTimer !== undefined) window.clearTimeout(resizeTimer)
    resizeTimer = window.setTimeout(() => {
      invoke('terminal_resize', { channelId: props.channelId, cols, rows }).catch(() => { })
    }, 150)
  })

  // 容器隐藏（后台标签 v-show 挂载）或布局未稳定（高度 0）时跳过初始 fit：
  // fit 会把 PTY resize 成 1x1/0 行，显示后由 ResizeObserver（尺寸 0 变为实际值）触发正确 fit
  if (terminalRef.value!.offsetWidth > 0 && terminalRef.value!.offsetHeight > 0) {
    fitAddon.fit()
    term.focus()

    // 第一次 fit 后手动发送 PTY 尺寸
    invoke('terminal_resize', {
      channelId: props.channelId,
      cols: term.cols,
      rows: term.rows,
    }).catch(() => { })
  }

  unlisten = await listen<any>('core-event', (event) => {
    try {
      // payload 为后端 emit 的事件对象（Tauri 已序列化传输，无需二次 parse）
      const parsed = event.payload
      if (parsed.type === 'Channel' && parsed.payload.kind === 'DataReceived') {
        if (parsed.payload.detail.channel_id === props.channelId) {
          // data 为 base64 编码的字节串（后端 serde_with::base64）
          const b64 = parsed.payload.detail.data as string
          const bin = atob(b64)
          const bytes = new Uint8Array(bin.length)
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
          term.write(bytes)
        }
      } else if (parsed.type === 'Channel' && parsed.payload.kind === 'Closed') {
        // 本通道已关闭（远端 exit 触发 EOF）：停止输入并显示提示（终端内文本，非 UI 字符串）
        if (parsed.payload.detail.channel_id === props.channelId && !ended) {
          ended = true
          term.write('\r\n\x1b[33m[Session ended]\x1b[0m\r\n')
        }
      }
    } catch (_) { }
  })

  // await listen 期间组件可能已卸载（旧通道被 :key 重建）：解绑监听器并中止初始化
  if (disposed) {
    unlisten()
    term.dispose()
    return
  }

  term.onData((data) => {
    // 会话已结束（exit 后通道关闭）：丢弃输入，不再发 IPC
    if (ended) return
    invoke('terminal_send_input', { channelId: props.channelId, data }).catch(() => { })
  })

  observer = new ResizeObserver(() => {
    // 隐藏标签（v-show 切换）容器尺寸归零：跳过 fit，防止后台 TUI 被 resize 成 1x1
    const el = terminalRef.value
    if (!el || el.offsetWidth === 0 || el.offsetHeight === 0) return
    fitAddon.fit()
  })
  observer.observe(terminalRef.value!)

  // xterm 和事件监听器就绪后，通知后端开始读取 SSH 数据
  await invoke('start_terminal', { channelId: props.channelId })
})

onBeforeUnmount(() => {
  disposed = true
  if (resizeTimer !== undefined) window.clearTimeout(resizeTimer)
  observer?.disconnect()
  unlisten?.()
  term?.dispose()
})
</script>

<template>
  <!-- 终端右键自定义菜单（MobaXterm 风格）；preventDefault 拦截浏览器默认菜单 -->
  <div ref="terminalRef" class="terminal-container" @contextmenu="onTermContextMenu">
    <div v-if="termMenu" class="context-menu" :style="termMenuStyle">
      <div class="menu-item" :class="{ disabled: !hasSelection }" @click="doCopy">{{ t('term.copy') }}</div>
      <div class="menu-item" @click="doPaste">{{ t('term.paste') }}</div>
      <div class="menu-item" @click="doClear">{{ t('term.clear') }}</div>
    </div>
  </div>
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

/* 终端右键菜单（与文件树/主机栏 context-menu 同风格） */
.context-menu {
  position: fixed;
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 4px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  z-index: 1000;
  min-width: 120px;
}

.menu-item {
  padding: 0.3rem 0.8rem;
  cursor: pointer;
  font-size: 0.8rem;
}

.menu-item:hover {
  background: var(--color-background-mute);
}

/* 复制项无选中文本时禁用 */
.menu-item.disabled {
  opacity: 0.4;
  cursor: default;
  pointer-events: none;
}
</style>
