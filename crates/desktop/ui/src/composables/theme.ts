// 界面主题切换（预留接口，与图标主题 fileIcon.ts 的预留对称）。
// 主题定义在 assets/base.css 的 :root[data-theme='x'] 变量块，切换仅需改
// document.documentElement 的 data-theme 属性（浏览器原生重算 CSS 变量，无 JS 开销）。
// 后续设置页接入：新增主题 = base.css 增加变量块 + 本表登记 id；持久化由设置功能负责

/** 可用主题 id 表（后续新增主题在此登记，设置页据此渲染选项） */
export const AVAILABLE_THEMES = ['onedark'] as const
export type ThemeId = (typeof AVAILABLE_THEMES)[number]

/** 当前主题 id（读 HTML 属性，单真相源；未知值回退默认主题） */
export function getTheme(): ThemeId {
  const t = document.documentElement.dataset.theme
  return (AVAILABLE_THEMES as readonly string[]).includes(t ?? '') ? (t as ThemeId) : 'onedark'
}

/** 切换主题（预留接口，后续设置页调用）；未知 id 不生效 */
export function setTheme(id: ThemeId): void {
  document.documentElement.dataset.theme = id
}
