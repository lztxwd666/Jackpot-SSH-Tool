// 浮层定位工具：fixed 定位浮层（右键菜单/悬停卡）防溢出窗口边缘

/** 按估算尺寸 clamp 浮层坐标（主机栏贴右缘时菜单右击溢出被裁剪的问题） */
export function clampFloatPos(x: number, y: number, width: number, height: number) {
  return {
    x: Math.max(8, Math.min(x, window.innerWidth - width - 8)),
    y: Math.max(8, Math.min(y, window.innerHeight - height - 8)),
  }
}
