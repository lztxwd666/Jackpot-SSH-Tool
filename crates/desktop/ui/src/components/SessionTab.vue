<script setup lang="ts">
// 单会话标签工作区：终端 + 双文件树 + 状态条（状态条由 Task B3 完善）
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

const props = defineProps<{ tab: SessionTabState }>()
const emit = defineEmits<{ (e: 'close'): void }>()

// 本地/远程文件树状态（每 tab 独立，v-show 切换保持）
const localCurrentDir = ref('')
const remoteCurrentDir = ref('/')
const localRefreshKey = ref(0)
const remoteRefreshKey = ref(0)
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
      <LocalFileTree :refreshKey="localRefreshKey" @current-dir="(p: string) => localCurrentDir = p" />
    </div>
    <div class="panel" style="width:180px; min-width:180px;">
      <RemoteFileTree :sessionId="tab.sessionId" :refreshKey="remoteRefreshKey" @current-dir="(p: string) => remoteCurrentDir = p" />
    </div>
  </div>
</template>

<style scoped>
.session-tab { display: flex; flex: 1; min-width: 0; }
.terminal-wrapper { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
.terminal-header { display: flex; justify-content: space-between; align-items: center; padding: 0.4rem 0.8rem; background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); flex-shrink: 0; }
.connection-info { font-size: 0.8rem; color: var(--color-heading); font-weight: 500; }
.panel { display: flex; flex-direction: column; overflow: hidden; }
</style>
