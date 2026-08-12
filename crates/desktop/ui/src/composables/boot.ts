// 启动就绪控制：窗口渲染完整后再显示（业内惯例，用户打开即完整 UI）。
// armBootFallback 在 Vue 挂载前调用——兜底覆盖 setup/onMounted 阶段失败或挂起，
// 窗口不会永久隐藏；completeBoot 在初始化完成后调用（取消兜底并显示窗口）。
// 模块级失败（import 链中断）时本模块不执行，窗口保持隐藏——已知限制
// （文档级兜底需 Tauri 内部 IPC 接口，收益不抵复杂度）

import { getCurrentWindow } from '@tauri-apps/api/window'

let bootTimer: number | undefined
let shown = false

function showWindow() {
  if (shown) return
  shown = true
  getCurrentWindow().show().catch(() => {})
}

/** 挂载前调用：窗口不可见期间设兜底超时（初始化失败/挂起时强制显示） */
export function armBootFallback(timeoutMs = 5000): void {
  bootTimer = window.setTimeout(showWindow, timeoutMs)
}

/** 初始化完成后调用：取消兜底并显示窗口 */
export function completeBoot(): void {
  if (bootTimer !== undefined) {
    window.clearTimeout(bootTimer)
    bootTimer = undefined
  }
  showWindow()
}
