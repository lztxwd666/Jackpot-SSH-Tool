<script setup lang="ts">
// 传输进度面板：展示所有进行中的下载/上传任务及实时进度

import { formatFileSize } from '../composables/fs'
import { t } from '../composables/i18n'

export interface TransferTask {
  id: string
  name: string
  direction: 'download' | 'upload'
  done: number
  total: number
  verifying?: boolean // 传输完成进入校验阶段（进度条显示"校验中"提示）
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
      <!-- 超长文件名省略号截断（保持进度条宽度），悬停 title 显示全名 -->
      <span class="t-name" :title="tr.name">{{ tr.direction === 'download' ? '↓' : '↑' }} {{ tr.name }}</span>
      <div class="t-bar">
        <div class="t-fill" :style="{ width: pct(tr) + '%' }"></div>
      </div>
      <!-- 校验阶段：显示"校验中"提示（大文件 SHA-256 校验耗时数秒，非卡住） -->
      <span class="t-pct">{{ tr.verifying ? t('transfer.verifying') : pct(tr) + '%' }}</span>
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
.t-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--color-text); }
.t-bar { flex: 2; height: 8px; background: var(--color-background-mute); border-radius: 4px; overflow: hidden; }
.t-fill {
  height: 100%; background: hsla(160, 100%, 37%, 1); border-radius: 4px;
  transition: width 0.2s;
}
.t-pct { width: 36px; text-align: right; color: var(--color-text); opacity: 0.8; }
.t-size { color: var(--color-text); opacity: 0.6; white-space: nowrap; }
</style>
