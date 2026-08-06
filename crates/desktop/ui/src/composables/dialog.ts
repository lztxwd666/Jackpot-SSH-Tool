// 全局对话框与 Toast 单例
// 替代原生 alert/confirm/prompt：原生弹窗是阻塞式的，会冻结整个 UI，
// 且 alert 只有确认没有取消，不符合可反悔的交互要求

import { reactive } from 'vue'
import { t } from './i18n'

export type DialogResult = boolean | string | null

interface DialogState {
  visible: boolean
  mode: 'confirm' | 'prompt'
  title: string
  message: string
  defaultValue: string
  resolve: ((result: DialogResult) => void) | null
}

export const dialogState = reactive<DialogState>({
  visible: false,
  mode: 'confirm',
  title: '',
  message: '',
  defaultValue: '',
  resolve: null,
})

/** 确认对话框：返回 Promise<boolean> */
export function confirmDialog(message: string, title = t('common.confirm')): Promise<boolean> {
  return new Promise((resolve) => {
    // 并发守卫：已有对话框打开时立即返回 false（取消语义），避免旧 Promise 永久挂起
    if (dialogState.visible) {
      resolve(false)
      return
    }
    dialogState.mode = 'confirm'
    dialogState.title = title
    dialogState.message = message
    dialogState.resolve = (r) => resolve(r === true)
    dialogState.visible = true
  })
}

/** 输入对话框：返回 Promise<string | null>，取消返回 null */
export function promptDialog(message: string, defaultValue = '', title = t('common.input')): Promise<string | null> {
  return new Promise((resolve) => {
    // 并发守卫：已有对话框打开时立即返回 null（取消语义）
    if (dialogState.visible) {
      resolve(null)
      return
    }
    dialogState.mode = 'prompt'
    dialogState.title = title
    dialogState.message = message
    dialogState.defaultValue = defaultValue
    dialogState.resolve = (r) => resolve(typeof r === 'string' ? r : null)
    dialogState.visible = true
  })
}

/** 关闭对话框并返回结果（取消 = null/false） */
export function closeDialog(result: DialogResult) {
  dialogState.resolve?.(result)
  dialogState.resolve = null
  dialogState.visible = false
}

// Toast 提示

export interface Toast {
  id: number
  message: string
  type: 'info' | 'success' | 'error' | 'warning'
}

let toastSeq = 0
export const toasts = reactive<Toast[]>([])

/** 非阻塞提示，自动消失 */
export function showToast(message: string, type: 'info' | 'success' | 'error' | 'warning' = 'info', duration = 3500) {
  const id = ++toastSeq
  toasts.push({ id, message, type })
  window.setTimeout(() => {
    const i = toasts.findIndex((t) => t.id === id)
    if (i >= 0) toasts.splice(i, 1)
  }, duration)
}
