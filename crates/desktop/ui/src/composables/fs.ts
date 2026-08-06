// 文件系统展示与校验工具（文件树与传输面板共用，避免各组件重复实现）

/** 字节数人类可读格式化（<1KB 显示 B，GB 级小数一位；原两棵树 0K/1536000K 劣化修复） */
export function formatFileSize(n: number): string {
  if (n >= 1024 * 1024 * 1024) return (n / (1024 * 1024 * 1024)).toFixed(1) + ' GB'
  if (n >= 1024 * 1024) return (n / (1024 * 1024)).toFixed(1) + ' MB'
  if (n >= 1024) return (n / 1024).toFixed(1) + ' KB'
  return n + ' B'
}

/**
 * 新建/重命名名称校验：拒绝空名、路径分隔符与精确 ..（自伤型路径穿越防护，
 * 输入 ..\..\x 会把目标建到/移动到当前目录之外；分隔符被拒后单独的 .. 段
 * 不可能出现，因此 a..b 这类含 .. 子串的合法文件名不受影响）
 */
export function isValidNewName(name: string): boolean {
  if (!name || name === '.' || name === '..') return false
  return !/[/\\]/.test(name)
}
