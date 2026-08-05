<script setup lang="ts">
// 小眼睛图标按钮：密码/口令明文切换（feather eye / eye-off 风格）
// 抽出为独立组件：密码与口令两处复用（初版在两处内联复制 SVG，违反 DRY，自查后抽取）
import { t } from '../composables/i18n'

defineProps<{ visible: boolean }>()
const emit = defineEmits<{ (e: 'toggle'): void }>()
</script>

<template>
  <button type="button" class="eye-toggle" :title="visible ? t('form.hideSecret') : t('form.showSecret')" @click="emit('toggle')">
    <svg v-if="!visible" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
    </svg>
    <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>
    </svg>
  </button>
</template>

<style scoped>
.eye-toggle {
  display: inline-flex; align-items: center; justify-content: center;
  padding: 0.3rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer;
}
.eye-toggle:hover { background: var(--color-background-mute); }
</style>
