// 面板拖拽调整（VSCode split view 惯例）：splitter mousedown 后跟踪指针移动，
// 以增量 dx 回调（调用方持有宽度状态并 clamp），释放/取消后清理。
// preventDefault 内建：阻止浏览器把按下后的移动解释为原生拖放/文本选择
// （splitter 与 draggable 树节点相邻，误命中会触发文件拖放）
// pointer capture：拖出窗口/失焦时 pointerup 仍派发到 capture 元素，杜绝监听器泄漏
// （无 capture 的 document 监听在窗口外释放时收不到 mouseup，残留监听导致
// 幽灵拖拽——面板随鼠标移动变形——与再次拖拽时 dx 倍速累积）
export function startPanelDrag(
  e: PointerEvent,
  onMove: (dx: number) => void,
  onDone: () => void,
): void {
  e.preventDefault()
  const target = e.target as HTMLElement
  let lastX = e.clientX
  let listenEl: EventTarget
  const move = (ev: Event) => {
    // EventTarget.addEventListener 回调参数为 Event，实际派发 PointerEvent（指针事件体系）
    const pe = ev as PointerEvent
    const dx = pe.clientX - lastX
    lastX = pe.clientX
    onMove(dx)
  }
  const finish = () => {
    listenEl.removeEventListener('pointermove', move)
    listenEl.removeEventListener('pointerup', finish)
    listenEl.removeEventListener('pointercancel', finish)
    document.body.style.cursor = ''
    onDone()
  }
  // capture 成功则监听绑定在 capture 元素（指针事件始终派发至此，窗口外释放也可靠）；
  // capture 失败（罕见）退化 document 级监听
  try {
    target.setPointerCapture(e.pointerId)
    listenEl = target
  } catch {
    listenEl = document
  }
  listenEl.addEventListener('pointermove', move)
  listenEl.addEventListener('pointerup', finish)
  listenEl.addEventListener('pointercancel', finish)
  document.body.style.cursor = 'col-resize'
}
