<script setup lang="ts">
// 传输进度面板：展示所有进行中的下载/上传任务及实时进度

import { formatFileSize } from '../composables/fs'

export interface TransferTask {
  id: string
  name: string
  direction: 'download' | 'upload'
  done: number
  total: number
  filename?: string // 目录传输当前文件相对路径（单文件传输为空）
}

defineProps<{ transfers: Record<string, TransferTask> }>()

function pct(t: TransferTask): number {
  if (!t.total) return 0
  return Math.min(100, Math.round((t.done / t.total) * 100))
}
</script>

<template>
  <div v-if="Object.keys(transfers).length > 0" class="transfer-panel">
    <div v-for="tr in Object.values(transfers)" :key="tr.id" class="transfer-item">
      <div class="t-main">
        <!-- 超长文件名省略号截断（保持进度条宽度），悬停 title 显示全名 -->
        <span class="t-name" :title="tr.name">{{ tr.direction === 'download' ? '↓' : '↑' }} {{ tr.name }}</span>
        <!-- 目录传输：当前文件相对路径（小字次显） -->
        <span v-if="tr.filename" class="t-file" :title="tr.filename">{{ tr.filename }}</span>
      </div>
      <div class="t-bar">
        <div class="t-fill" :style="{ width: pct(tr) + '%' }"></div>
      </div>
      <!-- 校验阶段提示为顶部 toast（绿色"校验中..."）；进度条此处保持纯百分比 -->
      <span class="t-pct">{{ pct(tr) }}%</span>
      <span class="t-size">{{ formatFileSize(tr.done) }} / {{ formatFileSize(tr.total) }}</span>
    </div>
  </div>
</template>

<style scoped>
.transfer-panel {
  position: fixed; bottom: 12px; right: 12px; z-index: 1900;
  display: flex; flex-direction: column; gap: 6px;
  background: var(--color-background); border: 1px solid var(--color-border);
  border-radius: 8px; padding: 10px; box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  min-width: 280px; max-width: 380px;
}
.transfer-item { display: flex; align-items: center; gap: 6px; font-size: 0.75rem; }
.t-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.t-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--color-text); }
.t-file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 0.68rem; color: var(--color-text); opacity: 0.55; }
.t-bar { flex: 2; height: 8px; background: var(--color-background-mute); border-radius: 4px; overflow: hidden; }
.t-fill {
  height: 100%; background: var(--color-accent); border-radius: 4px;
  transition: width 0.2s;
}
.t-pct { width: 36px; text-align: right; color: var(--color-text); opacity: 0.8; }
.t-size { color: var(--color-text); opacity: 0.6; white-space: nowrap; }
</style>
