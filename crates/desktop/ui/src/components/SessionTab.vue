<script setup lang="ts">
// 单会话标签工作区：终端 + 双文件树（状态条由 Task B3 完善）
// 传输函数在 App.vue 统一实现（全局 TransferPanel），本组件仅转发事件并携带 tab 粒度的目录状态
import { ref } from 'vue'
import Terminal from './Terminal.vue'
import LocalFileTree from './LocalFileTree.vue'
import RemoteFileTree from './RemoteFileTree.vue'
import { t } from '../composables/i18n'

export interface SessionTabState {
  id: string
  hostName: string
  sessionId: string
  channelId: string
  status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting'
  error?: string
}

const props = defineProps<{ tab: SessionTabState; localRefreshKey: number; remoteRefreshKey: number }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'download', remotePath: string, localDir?: string): void
  (e: 'upload', remoteDir: string, localPath: string, expectedDir?: string): void
}>()

// 本地/远程文件树当前目录（每 tab 独立，v-show 切换保持）
const localCurrentDir = ref('')
const remoteCurrentDir = ref('/')

// 本地文件树右键 "Upload to Remote"：上传到本标签的远程当前目录
function uploadFromLocal(localPath: string) {
  emit('upload', remoteCurrentDir.value || '/', localPath, localCurrentDir.value)
}
</script>

<template>
  <div class="session-tab">
    <div class="terminal-wrapper">
      <div class="terminal-header">
        <span class="connection-info">{{ tab.hostName }}</span>
        <button class="btn btn-danger" @click="emit('close')">{{ tab.status === 'connected' ? t('tab.disconnect') : t('tab.close') }}</button>
      </div>
      <!-- 状态条：Task B3 实现（v-if="tab.status !== 'connected'"） -->
      <Terminal v-if="tab.channelId" :channelId="tab.channelId" :key="tab.channelId" />
    </div>
    <div class="panel" style="width:180px; min-width:180px;">
      <LocalFileTree
        :refreshKey="localRefreshKey"
        @download="(p: string, dir: string) => emit('download', p, dir)"
        @current-dir="(p: string) => localCurrentDir = p"
        @upload-request="uploadFromLocal"
      />
    </div>
    <div class="panel" style="width:180px; min-width:180px;">
      <RemoteFileTree
        :sessionId="tab.sessionId"
        :refreshKey="remoteRefreshKey"
        @download="(p: string) => emit('download', p, localCurrentDir)"
        @upload="(dir: string, p: string) => emit('upload', dir, p, localCurrentDir)"
        @current-dir="(p: string) => remoteCurrentDir = p"
      />
    </div>
  </div>
</template>

<style scoped>
.session-tab { display: flex; flex: 1; min-width: 0; }
.terminal-wrapper { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.terminal-header { display: flex; justify-content: space-between; align-items: center; padding: 0.4rem 0.8rem; background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); flex-shrink: 0; }
.connection-info { font-size: 0.8rem; color: var(--color-heading); font-weight: 500; }
.panel { display: flex; flex-direction: column; overflow: hidden; }

.btn-danger {
  padding: 0.3rem 0.7rem; border: 1px solid #e5534b; border-radius: 4px;
  background: var(--color-background); color: #e5534b; cursor: pointer; font-size: 0.8rem;
}
.btn-danger:hover { background: #e5534b; color: #fff; }
</style>
