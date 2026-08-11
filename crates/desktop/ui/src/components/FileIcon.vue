<script setup lang="ts">
// 文件/文件夹类型图标：按当前图标主题解析素材并内联渲染（v-html 直出 SVG，零运行时依赖）
// 素材来自 vscode-material-icon-theme（MIT），来源标注与许可见 assets/icons/material/LICENSE
import { computed } from 'vue'
import { resolveFileIcon } from '../composables/fileIcon'

const props = withDefaults(defineProps<{
  /** 文件或目录名称（不含路径） */
  name: string
  /** 是否为目录 */
  isDir?: boolean
  /** 目录展开态（当前文件树为下钻式导航暂无展开态，预留后续可展开树使用） */
  open?: boolean
}>(), { isDir: false, open: false })

const svg = computed(() => resolveFileIcon(props.name, props.isDir, props.open))
</script>

<template>
  <span class="file-icon" v-html="svg" />
</template>

<style scoped>
.file-icon {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  height: 14px;
}
/* 素材仅带 viewBox 无固定尺寸，由容器统一缩放（16/24/32 viewBox 均等比显示） */
.file-icon :deep(svg) {
  width: 100%;
  height: 100%;
  display: block;
}
</style>
