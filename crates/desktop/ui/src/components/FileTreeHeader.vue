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
      <!-- 新建文件（SVG 无 emoji）：文件 + 加号 -->
      <button class="header-btn" :title="t('common.newFile')" @click="emit('new-file')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" /><path d="M12 11v6" /><path d="M9 14h6" />
        </svg>
      </button>
      <!-- 新建文件夹（SVG 无 emoji）：文件夹 + 加号 -->
      <button class="header-btn" :title="t('common.newFolder')" @click="emit('new-folder')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z" /><path d="M12 10v6" /><path d="M9 13h6" />
        </svg>
      </button>
      <!-- 刷新 -->
      <button class="header-btn" :title="t('common.refresh')" @click="emit('refresh')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
          <path d="M21 12a9 9 0 1 1-2.64-6.36M21 3v6h-6" />
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
