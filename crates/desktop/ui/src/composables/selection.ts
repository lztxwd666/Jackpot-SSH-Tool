// 文件树选区交互：失焦清除（VSCode/资源管理器行为）
// 选中文件树条目后点击其他区域（终端/主机栏等），选区应取消；
// 本 composable 供本地/远程两棵文件树共用

import { onMounted, onBeforeUnmount } from 'vue'

/**
 * 点击指定容器外部时清除选区（含右键点击）：组件挂载期间常驻监听，
 * 与菜单类"打开时监听"（useClickOutsideClose）语义不同
 * @param clear 清除回调（如 selected.clear + anchor 置空）
 * @param containerClass 容器选择器（如 '.file-tree'），点击其内不清除
 */
export function useClearSelectionOnOutside(clear: () => void, containerClass: string) {
  onMounted(() => {
    const handler = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(containerClass)) clear()
    }
    window.addEventListener('mousedown', handler)
    onBeforeUnmount(() => window.removeEventListener('mousedown', handler))
  })
}
