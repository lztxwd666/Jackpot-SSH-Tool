<script setup lang="ts">
// 全局 Toast 提示栈：直接渲染 dialog.ts 中的单例状态
import { toasts } from '../composables/dialog'
</script>

<template>
  <div class="toast-stack">
    <div v-for="t in toasts" :key="t.id" :class="['toast', t.type]">{{ t.message }}</div>
  </div>
</template>

<style scoped>
.toast-stack {
  position: fixed; top: 12px; left: 50%; transform: translateX(-50%);
  z-index: 2000; display: flex; flex-direction: column; gap: 6px; align-items: center;
  pointer-events: none;
}
.toast {
  padding: 8px 16px; border-radius: 6px; background: var(--color-background);
  border: 1px solid var(--color-border); box-shadow: 0 2px 12px rgba(0,0,0,0.35);
  font-size: 0.8rem; max-width: 70vw; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.toast.success { border-color: hsla(160, 100%, 37%, 0.6); color: hsla(160, 100%, 37%, 1); }
.toast.error { border-color: #e5534b; color: #e5534b; }
.toast.warning {
  border-color: #d29922; color: #d29922;
  background: rgba(210, 153, 34, 0.12);
  font-weight: 600;
}
/* 校验中（传输完成后的完整性校验阶段）：绿色常驻提示（duration 0，校验完由调用方移除） */
.toast.verifying {
  border-color: #2ea043; color: #2ea043;
  background: rgba(46, 160, 67, 0.12);
  font-weight: 600;
}
</style>
