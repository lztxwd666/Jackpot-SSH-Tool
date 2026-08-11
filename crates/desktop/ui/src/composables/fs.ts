// 文件系统展示与校验工具（文件树与传输面板共用，避免各组件重复实现）

import { choiceDialog } from './dialog'
import { t } from './i18n'

/**
 * 名称冲突处理（系统文件管理器惯例）：目标名已存在时弹窗询问 自动改名/覆盖，
 * 取消返回 null；自动改名为默认主操作（首个按钮）。两棵文件树共用
 * 冲突判定大小写不敏感（Windows NTFS）；allowOverwrite 为 false 时不提供
 * "覆盖"选项（重命名场景：Windows rename/SFTP rename 目标存在即失败，覆盖不可兑现）
 */
export async function resolveNameConflict(
  name: string,
  existing: Set<string>,
  options?: { allowOverwrite?: boolean },
): Promise<string | null> {
  const lower = new Set([...existing].map(s => s.toLowerCase()))
  if (!lower.has(name.toLowerCase())) return name
  const choices = [{ label: t('common.autoRename'), value: 'rename' }]
  if (options?.allowOverwrite !== false) {
    choices.push({ label: t('common.overwrite'), value: 'overwrite' })
  }
  const choice = await choiceDialog(t('prompt.overwriteMsg', { name }), t('prompt.overwriteTitle'), choices)
  if (!choice) return null
  return choice === 'rename' ? uniqueFileName(name, existing) : name
}

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

/**
 * 重名冲突时生成唯一名（系统文件管理器惯例）：preferred 被占用时追加 " (n)"
 * 于主名之后、扩展名之前，如 a.txt → a (1).txt → a (2).txt（检测已存在性递增）。
 * 点开头文件（.env）无扩展名，直接追加后缀
 * 已存在性按大小写不敏感比较（Windows NTFS 大小写不敏感：a.txt 与 A.txt 冲突，
 * 否则新建会静默截断原文件）
 */
export function uniqueFileName(preferred: string, existing: Set<string>): string {
  const lower = new Set([...existing].map(s => s.toLowerCase()))
  if (!lower.has(preferred.toLowerCase())) return preferred
  const dot = preferred.lastIndexOf('.')
  const base = dot > 0 ? preferred.slice(0, dot) : preferred
  const ext = dot > 0 ? preferred.slice(dot) : ''
  let i = 1
  while (lower.has(`${base} (${i})${ext}`.toLowerCase())) i++
  return `${base} (${i})${ext}`
}

/** 复制路径到剪贴板（WebView2 clipboard API）；失败返回 false 由调用方提示 */
export async function copyPath(path: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(path)
    return true
  } catch {
    return false
  }
}

/** 拖拽载荷条目（路径 + 是否目录） */
export interface DragItem {
  path: string
  isDir: boolean
}

/**
 * 解析文件树拖拽 payload（两棵树共用）：
 * 兼容三种格式——多选 { items: [{path,isDir}] } / 单选 { path, isDir } / 旧版纯路径字符串
 */
export function parseDragPayload(raw: string): DragItem[] {
  if (!raw) return []
  try {
    const p = JSON.parse(raw)
    if (Array.isArray(p.items)) {
      return p.items
        .filter((i: any) => i && typeof i.path === 'string')
        .map((i: any) => ({ path: i.path, isDir: !!i.isDir }))
    }
    if (p && typeof p.path === 'string') {
      return [{ path: p.path, isDir: !!p.isDir }]
    }
  } catch {
    // 非 JSON：旧版纯路径
  }
  return [{ path: raw, isDir: false }]
}
