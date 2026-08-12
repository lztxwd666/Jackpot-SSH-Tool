<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from '@xterm/addon-fit'
import { invoke } from '@tauri-apps/api/core'
import { registerChannelSink, unregisterChannelSink } from '../composables/channelData'
import { showToast } from '../composables/dialog'
import { clampFloatPos } from '../composables/pos'
import { useClickOutsideClose } from '../composables/menu'
import { t } from '../composables/i18n'
import 'xterm/css/xterm.css'

const props = defineProps<{ channelId: string }>()
const terminalRef = ref<HTMLDivElement>()
let term: Terminal
let fitAddon: FitAddon
let observer: ResizeObserver
let resizeTimer: number | undefined
let fitTimer: number | undefined
// 会话结束标记：shell EOF 远端关闭通道（Closed 事件）后停止发送输入并显示提示
// （否则输入继续走 IPC 报 channel not found 刷日志，终端表现为卡死）
let ended = false
// 挂载前数据缓冲：core-event 监听在组件创建时注册（setup 顶层，早于 onMounted），
// 消除竞态窗口——motd 等登录横幅在 shell 打开后立即到达，若等 onMounted 再注册
// 监听，数据 dispatch 时无消费者被丢弃（stdout 正常但最早的横幅丢失）。
// term 未就绪（xterm 尚未 open）前的数据先缓存，就绪后回放
const pendingData: Uint8Array[] = []
let termReady = false

function writeBytes(bytes: Uint8Array) {
  if (termReady) term.write(bytes)
  else pendingData.push(bytes)
}

function flushPending() {
  for (const d of pendingData) term.write(d)
  pendingData.length = 0
}

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

// 组件创建即注册通道写入器：数据由 App.vue 全局监听器统一分发（应用启动即注册，
// 无竞态窗口），组件创建前到达的早期数据（motd 等登录横幅）在 channelData 缓冲，
// 注册时回放；term 未就绪前 sink 内部继续缓冲，xterm open 后写入
registerChannelSink(props.channelId, {
  onData: writeBytes,
  onClosed: () => {
    // 本通道已关闭（远端 exit 触发 EOF）：停止输入并显示提示（终端内文本，非 UI 字符串）
    if (ended) return
    ended = true
    writeBytes(new TextEncoder().encode('\r\n\x1b[33m[Session ended]\x1b[0m\r\n'))
  },
})

onMounted(async () => {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    // 滚动缓冲：上限而非预分配——内存与实际输出行数线性（xterm 5.x 约 40B/字符：
    // 80 列行 ~3.2KB），日常会话（数千行）无论上限大小差异都在几 MB 内；只有超长
    // 输出才逼近上限。20000 行满载 ~64-96MB/终端，5 标签同时满载 ~500MB，有清晰
    // 安全边界（对齐主流：Windows Terminal ~9000 / mintty 10000 / VSCode 上限 50000）。
    // 默认 1000 行会截断长输出——长对话 TUI 重绘数万行时最早内容连同命令回显一起
    // 被移出缓冲；上限 50000 保留给未来设置页作为可配最大值（渲染性能不受缓冲大小
    // 影响，xterm 只渲染视口行；关标签 dispose 即释放全部缓冲）
    scrollback: 20000,
    // 终端配色独立于界面主题（OneDark ANSI 色板，实测自 Binaryify/OneDark-Pro，MIT）
    // 背景固定深色（OneDark-Pro 实测中间档 #23272e，比经典黑略淡但保持深色观感，
    // 用户反馈纯黑过于沉重），不随界面主题切换；其余色值为仓库实测值
    theme: {
      background: '#23272e', // 固定深色，不随主题切换
      foreground: '#abb2bf',
      cursor: '#61afef', // 仓库未定义 terminalCursor 键，采用设计值（OneDark 蓝）
      cursorAccent: '#23272e',
      selectionBackground: '#3e4451',
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
  // xterm 就绪：回放挂载前缓冲的早期数据（登录横幅等）
  termReady = true
  flushPending()

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

  term.onData((data) => {
    // 会话已结束（exit 后通道关闭）：丢弃输入，不再发 IPC
    if (ended) return
    invoke('terminal_send_input', { channelId: props.channelId, data }).catch(() => { })
  })

  observer = new ResizeObserver(() => {
    // 隐藏标签（v-show 切换）容器尺寸归零：跳过 fit，防止后台 TUI 被 resize 成 1x1
    const el = terminalRef.value
    if (!el || el.offsetWidth === 0 || el.offsetHeight === 0) return
    // fit 防抖：面板拖动/窗口缩放期间尺寸连续变化，每帧 fit 触发列数变化即全量重绘
    // 导致闪烁；变化停止 80ms 后一次性重排（VSCode 拖动分隔条同样延迟重排）。
    // 回调内复查尺寸：防抖窗口内容器可能被 v-show 隐藏（切标签），隐藏态 fit
    // 会把 PTY resize 成极小行列，复查跳过（重新显示时 observer 会再次触发）
    if (fitTimer !== undefined) window.clearTimeout(fitTimer)
    fitTimer = window.setTimeout(() => {
      const el = terminalRef.value
      if (!el || el.offsetWidth === 0 || el.offsetHeight === 0) return
      fitAddon.fit()
    }, 80)
  })
  observer.observe(terminalRef.value!)

  // xterm 和事件监听器就绪后，通知后端开始读取 SSH 数据
  await invoke('start_terminal', { channelId: props.channelId })
})

onBeforeUnmount(() => {
  unregisterChannelSink(props.channelId)
  if (resizeTimer !== undefined) window.clearTimeout(resizeTimer)
  if (fitTimer !== undefined) window.clearTimeout(fitTimer)
  observer?.disconnect()
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
  /* 终端面板背景延伸到容器边缘：字符与边框之间的留白区与终端同色
     （而非露出外层框架色形成"挖缝"），整体呈完整深色面板 */
  background: #23272e;
}

.terminal-container :deep(.xterm) {
  height: 100%;
  /* 内容四周统一内边距（参照文件树行内边距惯例）：fit 读取 .xterm 自身
     padding 扣除后计算行列，字符与面板边缘留白一致，不会溢出 */
  padding: 8px;
}

.terminal-container :deep(.xterm-viewport) {
  overflow-y: auto;
  /* 滚动条区背景透明：xterm 默认给滚动条预留 14px 且 viewport 背景为不透明黑，
     改透明后该区域与面板背景同色，滚动条浮层呈"隔离在右侧"而非覆盖在文本上 */
  background-color: transparent;
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
