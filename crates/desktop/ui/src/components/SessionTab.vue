<script setup lang="ts">
// 单会话标签工作区：终端 + 双文件树 + 状态条（断连显示原因 + 手动重连按钮）
// 传输函数在 App.vue 统一实现（全局 TransferPanel），本组件仅转发事件并携带 tab 粒度的目录状态
import { ref } from 'vue'
import Terminal from './Terminal.vue'
import LocalFileTree from './LocalFileTree.vue'
import RemoteFileTree from './RemoteFileTree.vue'
import { t } from '../composables/i18n'

export interface SessionTabState {
  id: string
  // 按 hostId 关联主机（name 非唯一且可重命名，hostId 才是稳定标识；Task 9 由 hostName 匹配迁移而来）
  hostId: string
  hostName: string
  // 主机地址（建标签时快照，终端头部展示用；主机改名/改址后标签保留旧值，与 hostName 语义一致）
  address: string
  sessionId: string
  channelId: string
  status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting'
  error?: string
  // 传输进行中锁定远程文件树（按 tab 粒度，Task 7 从旧全局 remoteTreeLocked 迁移而来）
  locked?: boolean
  // 状态条提示列表（事件驱动 upsert/remove，可扩展：新提示 = 新事件映射，渲染零改动）
  notices: TabNotice[]
  // 关闭取消标记：closeTab 置位，进行中的重连流程在各 await 检查点中止（迟到成功不操作已关闭标签）
  cancelled: boolean
}

// 状态条提示（terminal 顶部）：统一模型，事件分派器按 id 增删
// 新提示只需在分派器加一条事件映射（如未来"端口转发中"），无需改渲染
// 操作按钮不放在提示里（用户反馈：重连按钮集中在断连遮罩中央，状态条仅提示状态）
export interface TabNotice {
  id: string            // 稳定标识（upsert/remove 用，如 'transfer-busy'）
  level: 'info' | 'warning' | 'error' | 'verifying'
  message: string       // 已翻译文案（t() 生成）
}

const props = defineProps<{ tab: SessionTabState; localRefreshKey: number; remoteRefreshKey: number; locked?: boolean }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'reconnect'): void
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
  <!-- 布局顺序：本地文件树 | 远程文件树 | 终端（恢复 Stage 5 原设计；Task 5 曾误写为终端在左） -->
  <div class="session-tab">
    <!-- 断连遮罩（用户反馈）：断开后文件树与终端各自遮罩提示，不可操作；
         终端内容保留可见（断开前输出可回看）；重连按钮集中在终端遮罩 -->
    <div class="panel panel-relative" style="width:180px; min-width:180px;">
      <LocalFileTree
        :refreshKey="localRefreshKey"
        @download="(p: string, dir: string) => emit('download', p, dir)"
        @current-dir="(p: string) => localCurrentDir = p"
        @upload-request="uploadFromLocal"
      />
      <div v-if="tab.status === 'disconnected'" class="disconnect-overlay"><span>{{ t('tab.overlayDisconnected') }}</span></div>
    </div>
    <div class="panel panel-relative" style="width:180px; min-width:180px;">
      <RemoteFileTree
        :sessionId="tab.sessionId"
        :refreshKey="remoteRefreshKey"
        :locked="locked"
        @download="(p: string) => emit('download', p, localCurrentDir)"
        @upload="(dir: string, p: string) => emit('upload', dir, p, localCurrentDir)"
        @current-dir="(p: string) => remoteCurrentDir = p"
      />
      <div v-if="tab.status === 'disconnected'" class="disconnect-overlay"><span>{{ t('tab.overlayDisconnected') }}</span></div>
    </div>
    <div class="terminal-wrapper">
      <!-- 终端头部：主机名 + IP 地址（用户反馈：仅主机名过于单调，添加地址更直观）；
           断连/重连按钮不在此处（用户反馈：遮罩中央的重连按钮方案更协调，断开操作走标签栏 × 关闭） -->
      <div class="terminal-header">
        <span class="connection-info">
          <span class="connection-host">{{ tab.hostName }}</span>
          <span class="connection-address">{{ tab.address }}</span>
        </span>
      </div>
      <!-- 状态条：notices 统一渲染（可多条堆叠；操作按钮集中断连遮罩，状态条仅提示） -->
      <div v-if="tab.notices.length" class="notice-stack">
        <div v-for="n in tab.notices" :key="n.id" class="status-banner" :class="n.level">
          <!-- 级别图标（SVG 无 emoji）：info/warning/error 共用感叹号圆标，颜色随 level 着色 -->
          <svg class="notice-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10" /><path d="M12 8v4" /><path d="M12 16h.01" />
          </svg>
          <span class="notice-message">{{ n.message }}</span>
        </div>
      </div>
      <div class="terminal-body">
        <Terminal v-if="tab.channelId" :channelId="tab.channelId" :key="tab.channelId" />
        <!-- 终端遮罩：断开时覆盖终端区（内容仍可见），中央提示 + 重连按钮 -->
        <div v-if="tab.status === 'disconnected'" class="disconnect-overlay terminal-overlay">
          <span class="overlay-title">{{ t('tab.overlayDisconnected') }}</span>
          <button class="btn btn-primary" @click="emit('reconnect')">{{ t('tab.reconnect') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.session-tab { display: flex; flex: 1; min-width: 0; }
/* 终端与远程文件树之间的分隔线（线条对齐：各区域分隔统一 1px var(--color-border)） */
.terminal-wrapper { flex: 1; display: flex; flex-direction: column; overflow: hidden; border-left: 1px solid var(--color-border); }
.terminal-header { display: flex; justify-content: space-between; align-items: center; padding: 0.4rem 0.8rem; background: var(--color-background-soft); border-bottom: 1px solid var(--color-border); flex-shrink: 0; }
.connection-info { display: inline-flex; align-items: baseline; gap: 0.5rem; min-width: 0; }
.connection-host { font-size: 0.8rem; color: var(--color-heading); font-weight: 500; white-space: nowrap; }
/* IP 地址：次显（灰色小字），与主机名区分层级 */
.connection-address { font-size: 0.72rem; color: var(--color-text); opacity: 0.55; font-family: Consolas, monospace; white-space: nowrap; }
/* 状态条（notices）：按 level 着色；多条堆叠（notice-stack）；图标脉冲动画表达"进行中" */
.notice-stack { border-bottom: 1px solid var(--color-border); }
.status-banner { padding: 0.35rem 0.8rem; font-size: 0.78rem; display: flex; align-items: center; gap: 0.5rem; animation: banner-in 0.18s ease-out; }
.status-banner.info { background: rgba(88, 166, 255, 0.1); color: var(--color-text); }
.status-banner.warning { background: rgba(210, 153, 34, 0.12); color: #d29922; }
.status-banner.error { background: rgba(229, 83, 75, 0.12); color: #e5534b; }
/* 校验中（传输完成后的完整性校验阶段）：绿色表达"即将完成"的积极状态 */
.status-banner.verifying { background: rgba(46, 160, 67, 0.12); color: #2ea043; }
.notice-icon { width: 12px; height: 12px; flex-shrink: 0; animation: icon-pulse 1.6s ease-in-out infinite; }
.notice-message { flex: 1; }
@keyframes banner-in { from { opacity: 0; transform: translateY(-2px); } to { opacity: 1; transform: none; } }
@keyframes icon-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }
.panel { display: flex; flex-direction: column; overflow: hidden; }
/* 断连遮罩容器：文件树/终端区域定位上下文 */
.panel-relative, .terminal-body { position: relative; }
.terminal-body { flex: 1; display: flex; overflow: hidden; }
/* 断连遮罩：半透明覆盖各自区域（内容保留可见），中央提示；禁止交互 */
.disconnect-overlay {
  position: absolute; inset: 0; background: rgba(0, 0, 0, 0.45);
  display: flex; align-items: center; justify-content: center;
  color: var(--color-text); font-size: 0.8rem; z-index: 5;
  pointer-events: auto;
}
.terminal-overlay { flex-direction: column; gap: 0.6rem; }
.overlay-title { color: #e5534b; font-weight: 600; }

/* 本组件内按钮样式（App.vue scoped 样式不作用于此，Task 6 的 .btn-danger 同例） */
.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn-primary { background: hsla(160, 100%, 37%, 1); color: #fff; border-color: hsla(160, 100%, 37%, 1); }
.btn-primary:hover { background: hsla(160, 100%, 30%, 1); }

.btn-danger {
  padding: 0.3rem 0.7rem; border: 1px solid #e5534b; border-radius: 4px;
  background: var(--color-background); color: #e5534b; cursor: pointer; font-size: 0.8rem;
}
.btn-danger:hover { background: #e5534b; color: #fff; }
</style>
