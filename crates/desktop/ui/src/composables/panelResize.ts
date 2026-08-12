// 面板拖拽调整（VSCode split view 惯例）：splitter mousedown 后 document 级跟踪移动，
// 以增量 dx 回调（调用方持有宽度状态并 clamp），mouseup 释放并清理。
// 增量式避免拖拽锚点计算（面板左缘随布局变化时仍稳定）
export function startPanelDrag(
  startX: number,
  onMove: (dx: number) => void,
  onDone: () => void,
): void {
  let lastX = startX
  const move = (e: MouseEvent) => {
    const dx = e.clientX - lastX
    lastX = e.clientX
    onMove(dx)
  }
  const up = () => {
    document.removeEventListener('mousemove', move)
    document.removeEventListener('mouseup', up)
    document.body.style.cursor = ''
    onDone()
  }
  document.addEventListener('mousemove', move)
  document.addEventListener('mouseup', up)
  document.body.style.cursor = 'col-resize'
}
