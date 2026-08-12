// 数值与浮层定位工具

/** 数值边界约束（拖拽宽度/持久化初始化共用；拖拽三处与初始化三处同一语义） */
export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v))
}

/** 按估算尺寸 clamp 浮层坐标（主机栏贴右缘时菜单右击溢出被裁剪的问题） */
export function clampFloatPos(x: number, y: number, width: number, height: number) {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  }
}
