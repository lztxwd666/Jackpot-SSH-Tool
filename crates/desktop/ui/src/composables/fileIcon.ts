// 文件图标主题解析（两棵文件树共用）。
// 内置 material 主题：素材与映射来自 vscode-material-icon-theme（MIT），
// 来源标注与完整许可见 assets/icons/material/LICENSE；映射清单由
// scripts/fetch_material_icons.py 生成（单一来源），如需增删图标改脚本后重跑。
// 预留多主题接口：后续设置功能通过 registerFileIconTheme / setFileIconTheme
// 注册与切换主题（currentThemeId 为响应式，切换后所有 FileIcon 自动重渲染）。

import { ref } from 'vue'
import materialManifest from '../assets/icons/themes/material/manifest'

/**
 * 图标主题契约：主题只需回答"某个名称用什么图标"。
 * 返回素材名（如 'typescript'、'folder-src-open'），null 表示无专属图标，由调用方回落到默认图标。
 * 素材按主题目录组织（assets/icons/themes/{assetsDir}/），新增主题注册时声明自己的目录。
 */
export interface FileIconTheme {
  /** 主题标识（后续设置持久化时作为存储键） */
  readonly id: string
  /** 素材目录名（assets/icons/themes/ 下的子目录名，如 'material'） */
  readonly assetsDir: string
  /** 按文件名解析文件图标（扩展名/文件名/配置类前缀匹配），null 走默认文件图标 */
  resolveFile(name: string): string | null
  /** 按目录名解析文件夹图标，open 为展开态；null 走默认文件夹图标 */
  resolveFolder(name: string, open: boolean): string | null
}

// 素材以 raw 字符串内联进 bundle（桌面应用离线可用）；键为相对本模块的素材路径。
// glob 为构建期静态展开：新增主题时把其素材目录并入此通配（如 {material,newtheme}）
const svgFiles = import.meta.glob('../assets/icons/themes/material/{file,folder}/*.svg', {
  query: '?raw', import: 'default', eager: true,
}) as Record<string, string>

const themes = new Map<string, FileIconTheme>()
/** 当前主题 id（响应式；后续设置功能写入并持久化） */
const currentThemeId = ref('material')

/** 注册图标主题（模块加载时调用；后续设置页引入新主题包时调用） */
export function registerFileIconTheme(theme: FileIconTheme): void {
  themes.set(theme.id, theme)
}

/** 切换当前图标主题（预留接口，后续设置功能调用）；未知 id 返回 false */
export function setFileIconTheme(id: string): boolean {
  if (!themes.has(id)) return false
  currentThemeId.value = id
  return true
}

// material 主题实现：直接消费脚本生成的映射清单，与素材保持一致
const BY_EXT = materialManifest.byExtension as Record<string, string>
const BY_NAME = materialManifest.byName as Record<string, string>
const BY_PREFIX = materialManifest.byPrefix as Record<string, string>
const FOLDER_BY_NAME = materialManifest.folderByName as Record<string, string>

const materialTheme: FileIconTheme = {
  id: 'material',
  assetsDir: 'material',
  resolveFile(name) {
    const lower = name.toLowerCase()
    // 点开头文件名先剥点再查精确名（.gitignore → gitignore；.env 本身在精确名表）
    const stem = lower.startsWith('.') ? lower.slice(1) : lower
    const exact = BY_NAME[lower] ?? BY_NAME[stem]
    if (exact) return exact
    // 配置类前缀匹配（.eslintrc.json、vite.config.ts、.d.ts 等带后缀变体的文件名）
    for (const [prefix, icon] of Object.entries(BY_PREFIX)) {
      if (lower.startsWith(prefix)) return icon
    }
    // 扩展名：点开头文件取首点后段（.env.local 的 local 无命中，由前缀表兜住 .env）
    const base = lower.startsWith('.') ? lower.slice(1) : lower
    const dot = base.lastIndexOf('.')
    const ext = dot >= 0 ? base.slice(dot + 1) : ''
    return BY_EXT[ext] ?? null
  },
  resolveFolder(name, open) {
    const lower = name.toLowerCase()
    // 上游惯例：目录名首部点/下划线/连字符视为同义变体（.config、_src、-src 命中同图标）
    const key = lower.replace(/^[._-]+/, '')
    const icon = FOLDER_BY_NAME[lower] ?? FOLDER_BY_NAME[key] ?? null
    if (!icon) return null
    return open ? `${icon}-open` : icon
  },
}
registerFileIconTheme(materialTheme)

// 兜底内联图标：仅在素材缺失（构建异常/未来主题引用不存在的素材）时出现，避免空渲染
const FALLBACK_FILE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#8b949e" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /></svg>'
const FALLBACK_FOLDER_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#d29922" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /></svg>'

/** 解析文件/文件夹图标 SVG（未知类型回落到主题默认图标，素材缺失回落到内联兜底） */
export function resolveFileIcon(name: string, isDir: boolean, open: boolean): string {
  const theme = themes.get(currentThemeId.value) ?? materialTheme
  const key = isDir ? theme.resolveFolder(name, open) : theme.resolveFile(name)
  // 素材名以 folder 开头者为文件夹图标（folder/ 目录），其余为文件图标（file/ 目录）
  const dir = key?.startsWith('folder') ? 'folder' : 'file'
  const base = `../assets/icons/themes/${theme.assetsDir}/`
  return (
    svgFiles[`${base}${dir}/${key ?? ''}.svg`] ??
    svgFiles[`${base}${isDir ? (open ? 'folder/folder-open' : 'folder/folder') : 'file/file'}.svg`] ??
    (isDir ? FALLBACK_FOLDER_SVG : FALLBACK_FILE_SVG)
  )
}
