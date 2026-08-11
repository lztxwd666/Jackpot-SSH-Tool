<script setup lang="ts">
// 文件树标题栏（VSCode EXPLORER 样式）：标题居左，右侧新建文件/新建文件夹/刷新
// 图标正常隐藏，悬停标题栏时淡入（纯 CSS hover，交互跟随 VSCode 惯例）
import { t } from '../composables/i18n'

defineProps<{ title: string }>()
const emit = defineEmits<{
  (e: 'new-file'): void
  (e: 'new-folder'): void
  (e: 'refresh'): void
}>()
</script>

<template>
  <div class="tree-header">
    <span class="header-title">{{ title }}</span>
    <span class="header-actions">
      <!-- 固定动作图标（vscode-codicons，MIT 许可，path 内联，VSCode explorer 同款）：
           不随图标主题替换，保证动作可识别性 -->
      <button class="header-btn" :title="t('common.newFile')" @click="emit('new-file')">
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M5 14C4.448 14 4 13.552 4 13V3C4 2.448 4.448 2 5 2H8V4.5C8 5.328 8.672 6 9.5 6H12V6.025C12.344 6.056 12.677 6.121 13 6.213V5.414C13 5.016 12.842 4.635 12.561 4.353L9.647 1.439C9.366 1.158 8.984 1 8.586 1H5C3.895 1 3 1.895 3 3V13C3 14.105 3.895 15 5 15H7.261C7.008 14.693 6.791 14.357 6.607 14H5ZM9 2.207L11.793 5H9.5C9.224 5 9 4.776 9 4.5V2.207ZM11.5 7C9.015 7 7 9.015 7 11.5C7 13.985 9.015 16 11.5 16C13.985 16 16 13.985 16 11.5C16 9.015 13.985 7 11.5 7ZM14 12H12V14C12 14.276 11.776 14.5 11.5 14.5C11.224 14.5 11 14.276 11 14V12H9C8.724 12 8.5 11.776 8.5 11.5C8.5 11.224 8.724 11 9 11H11V9C11 8.724 11.224 8.5 11.5 8.5C11.776 8.5 12 8.724 12 9V11H14C14.276 11 14.5 11.224 14.5 11.5C14.5 11.776 14.276 12 14 12Z" />
        </svg>
      </button>
      <button class="header-btn" :title="t('common.newFolder')" @click="emit('new-folder')">
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M2 4.5V6H5.58579C5.71839 6 5.84557 5.94732 5.93934 5.85355L7.29289 4.5L5.93934 3.14645C5.84557 3.05268 5.71839 3 5.58579 3H3.5C2.67157 3 2 3.67157 2 4.5ZM1 4.5C1 3.11929 2.11929 2 3.5 2H5.58579C5.98361 2 6.36514 2.15804 6.64645 2.43934L8.20711 4H12.5C13.8807 4 15 5.11929 15 6.5V7.25716C14.6929 7.00353 14.3578 6.78261 14 6.59971V6.5C14 5.67157 13.3284 5 12.5 5H8.20711L6.64645 6.56066C6.36514 6.84197 5.98361 7 5.58579 7H2V11.5C2 12.3284 2.67157 13 3.5 13H6.20703C6.30564 13.3486 6.43777 13.6832 6.59971 14H3.5C2.11929 14 1 12.8807 1 11.5V4.5ZM16 11.5C16 13.9853 13.9853 16 11.5 16C9.01472 16 7 13.9853 7 11.5C7 9.01472 9.01472 7 11.5 7C13.9853 7 16 9.01472 16 11.5ZM12 9C12 8.72386 11.7761 8.5 11.5 8.5C11.2239 8.5 11 8.72386 11 9V11H9C8.72386 11 8.5 11.2239 8.5 11.5C8.5 11.7761 8.72386 12 9 12H11V14C11 14.2761 11.2239 14.5 11.5 14.5C11.7761 14.5 12 14.2761 12 14V12H14C14.2761 12 14.5 11.7761 14.5 11.5C14.5 11.2239 14.2761 11 14 11H12V9Z" />
        </svg>
      </button>
      <button class="header-btn" :title="t('common.refresh')" @click="emit('refresh')">
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M3 8C3 5.23858 5.23858 3 8 3C9.63527 3 11.0878 3.78495 12.0005 5H10C9.72386 5 9.5 5.22386 9.5 5.5C9.5 5.77614 9.72386 6 10 6H12.8904C12.8973 6.00014 12.9041 6.00014 12.911 6H13C13.2761 6 13.5 5.77614 13.5 5.5V2.5C13.5 2.22386 13.2761 2 13 2C12.7239 2 12.5 2.22386 12.5 2.5V4.03138C11.4009 2.78613 9.79253 2 8 2C4.68629 2 2 4.68629 2 8C2 11.3137 4.68629 14 8 14C11.1301 14 13.6999 11.6035 13.9756 8.54488C14.0003 8.26985 13.7975 8.0268 13.5225 8.00202C13.2474 7.97723 13.0044 8.1801 12.9796 8.45512C12.75 11.003 10.6079 13 8 13C5.23858 13 3 10.7614 3 8Z" />
        </svg>
      </button>
    </span>
  </div>
</template>

<style scoped>
/* 标题居左；右侧动作区默认隐藏，悬停标题栏时淡入（VSCode EXPLORER 惯例） */
.tree-header {
  display: flex; align-items: center; justify-content: space-between;
  padding: 0.35rem 0.5rem; font-weight: 600; color: var(--color-heading);
  border-bottom: 1px solid var(--color-border); flex-shrink: 0;
}
.header-title { font-size: 0.8rem; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.header-actions {
  display: flex; align-items: center; gap: 2px;
  opacity: 0; transition: opacity 0.15s;
}
.tree-header:hover .header-actions { opacity: 1; }
.header-btn {
  display: inline-flex; align-items: center; justify-content: center;
  width: 20px; height: 20px; padding: 0; border: none; border-radius: 3px;
  background: transparent; color: var(--color-text); opacity: 0.7; cursor: pointer;
}
.header-btn:hover { background: var(--color-background-mute); opacity: 1; }
.header-btn svg { width: 13px; height: 13px; }
</style>
