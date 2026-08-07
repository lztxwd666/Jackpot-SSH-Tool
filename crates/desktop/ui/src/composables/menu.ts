// 自定义浮层菜单的通用交互（右键菜单/悬停卡等 position: fixed 浮层共用）

import { watch, type Ref } from 'vue'

/**
 * 点击浮层外部关闭：浮层打开（open 变为 truthy）时注册全局 mousedown 监听，
 * 点击浮层内部（.context-menu 容器）不关闭（菜单项 click 自行处理），
 * 点击其他任意位置关闭。解决"菜单必须选一项才能消除"的交互缺陷
 */
export function useClickOutsideClose(open: Ref<unknown>, onClose: () => void) {
  let handler: ((e: MouseEvent) => void) | null = null
  watch(open, (v) => {
    if (v) {
      handler = (e: MouseEvent) => {
        if ((e.target as HTMLElement).closest('.context-menu')) return
        onClose()
      }
      window.addEventListener('mousedown', handler)
    } else if (handler) {
      window.removeEventListener('mousedown', handler)
      handler = null
    }
  })
}
