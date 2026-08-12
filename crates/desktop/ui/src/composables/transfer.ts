// 传输编排（下载 / 上传 / 批量传输 / 进度面板状态）
// 从 App.vue 拆分：传输任务状态与标签会话经 deps 注入解耦；关闭标签经
// cancelSessionTransfers 取消该会话进行中的任务（进度条立即移除，迟到结果静默）

import { ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { removeToast, showToast } from './dialog'
import { t } from './i18n'
import type { DragItem } from './fs'
import type { SessionTabState } from '../components/SessionTab.vue'

export interface TransferTask {
  id: string
  sessionId: string // 归属会话：关闭标签时据此取消该会话进行中的传输
  name: string
  direction: 'download' | 'upload'
  done: number
  total: number
  filename?: string // 目录传输当前文件相对路径（单文件传输为空）
}

/** 依赖注入：传输编排需要的外部状态（App.vue 提供，避免 composable 持有全局单例） */
export interface UseTransfersDeps {
  /** 标签列表（批量传输中途会话被关闭时中止剩余项） */
  tabs: Ref<SessionTabState[]>
  /** 用户主目录（下载兜底目标；App.vue 启动时赋值） */
  homeDir: Ref<string>
}

export function useTransfers(deps: UseTransfersDeps) {
  const { tabs, homeDir } = deps

  const transfers = ref<Record<string, TransferTask>>({})
  const downloading = ref<Record<string, boolean>>({})  // 下载防重入守卫
  const uploading = ref<Record<string, boolean>>({})    // 上传防重入守卫

  // 已取消的传输（taskId）：closeTab 关闭会话时置位，迟到结果静默（不弹失败 toast、不刷新树）
  const cancelledTransfers = new Set<string>()

  // 校验中 toast（taskId → toast id）：传输完成进入校验时弹出绿色常驻提示，
  // 校验完成（成功/失败/取消）时移除，随后"已下载到"等结果 toast 同位置衔接
  const verifyingToasts: Record<string, number> = {}

  // 每标签刷新令牌：下载完成刷新对应标签的本地文件树，上传完成刷新远程文件树
  const localRefresh = ref<Record<string, number>>({})
  const remoteRefresh = ref<Record<string, number>>({})
  function bumpLocalRefresh(sessionId: string) { localRefresh.value[sessionId] = (localRefresh.value[sessionId] ?? 0) + 1 }
  function bumpRemoteRefresh(sessionId: string) { remoteRefresh.value[sessionId] = (remoteRefresh.value[sessionId] ?? 0) + 1 }

  // 批量传输（多选）：逐项串行执行（worker 单线程 transferring 互斥，
  // 并发发起会报 transfer already in progress）；单文件失败不影响后续；
  // 批量中途会话被关闭（标签关闭）时中止剩余项，避免对已关会话报错刷 toast
  async function downloadMany(sessionId: string, items: DragItem[], localDir?: string) {
    for (const item of items) {
      if (!tabs.value.some(t => t.sessionId === sessionId)) return
      await downloadFile(sessionId, item.path, localDir, item.isDir)
    }
  }
  async function uploadMany(sessionId: string, remoteDir: string, items: DragItem[], expectedDir?: string) {
    for (const item of items) {
      if (!tabs.value.some(t => t.sessionId === sessionId)) return
      await uploadFile(sessionId, remoteDir, item.path, expectedDir, item.isDir)
    }
  }

  // 关闭标签时取消该会话进行中的传输：进度条立即移除（后端 Close 命令同时中止 worker
  // 传输），迟到结果静默——断开即取消，传输不得在会话关闭后继续跑
  function cancelSessionTransfers(sessionId: string) {
    for (const [tid, tr] of Object.entries(transfers.value)) {
      if (tr.sessionId === sessionId) {
        cancelledTransfers.add(tid)
        delete transfers.value[tid]
        // 同步移除该传输的"校验中"toast（若已进入校验阶段）
        const vt = verifyingToasts[tid]
        if (vt !== undefined) {
          removeToast(vt)
          delete verifyingToasts[tid]
        }
      }
    }
  }

  // 传输进度事件处理（App.vue 的 transfer-progress 监听器调用）：
  // 更新任务进度；传输完成进入校验时弹出绿色常驻 toast（移除由 downloadFile/uploadFile
  // 的 finally 承担，随后"已下载到"等结果 toast 同位置衔接）。同一传输重复事件幂等；
  // 守卫：传输已结束（进度条已移除）或已被取消时不弹——事件与命令回复经不同 IPC
  // 通道，顺序无强保证，迟到事件不得产生无 remove 路径的常驻 toast
  function handleTransferProgress(p: { id: string; done: number; total: number; verifying?: boolean; filename?: string }) {
    const tr = transfers.value[p.id]
    if (tr) {
      tr.done = p.done
      tr.total = p.total
      tr.filename = p.filename ?? ''
    }
    if (
      p.verifying
      && transfers.value[p.id]
      && !cancelledTransfers.has(p.id)
      && verifyingToasts[p.id] === undefined
    ) {
      verifyingToasts[p.id] = showToast(t('transfer.verifying'), 'verifying', 0)
    }
  }

  // SFTP 操作（按 tab 的 sessionId 调用）
  // 下载目标优先级：拖拽目标目录 > 本地文件树当前目录（tab 内）> 用户主目录\Downloads > 用户主目录
  // 注意：C:\Users 根目录因 UAC 权限限制不可写，切勿作为默认目标
  async function downloadFile(sessionId: string, remotePath: string, localDir?: string, isDir = false) {
    if (!sessionId) return
    // 防重入：同一文件/目录正在下载时忽略重复点击
    if (downloading.value[remotePath]) {
      showToast(t('toast.downloadInProgress'), 'info')
      return
    }
    downloading.value[remotePath] = true
    // 清洗文件名：替换 Windows 非法字符与路径分隔符，拒绝纯点（. 和 ..），防路径穿越
    const rawName = remotePath.split('/').pop() || 'download'
    const fileName = rawName.replace(/[\\/:*?"<>|]/g, '_').replace(/^\.+$/, '_')
    let dir = localDir || ''
    if (!dir && homeDir.value) {
      dir = homeDir.value
      // 优先保存到 Downloads 目录（如果存在）
      try {
        await invoke('read_local_dir', { path: homeDir.value + '\\Downloads' })
        dir = homeDir.value + '\\Downloads'
      } catch (_) {}
    }
    const localPath = dir.replace(/\\$/, '') + '\\' + fileName

    // 创建传输任务（进度面板显示）
    const taskId = crypto.randomUUID()
    transfers.value[taskId] = { id: taskId, sessionId, name: fileName, direction: 'download', done: 0, total: 0 }
    try {
      // 目录走递归传输命令（进度按文件粒度，不做逐文件校验）；单文件保留 SHA-256 校验
      if (isDir) {
        await invoke('sftp_download_tree', {
          sessionId, remotePath, localPath, taskId, expectedDir: dir,
        })
      } else {
        await invoke('sftp_download_file', {
          sessionId, remotePath, localPath, taskId, expectedDir: dir,
        })
      }
      if (cancelledTransfers.has(taskId)) return // 会话已关闭：迟到结果静默
      // 刷新对应标签的本地文件树
      bumpLocalRefresh(sessionId)
      showToast(t('toast.downloaded', { path: localPath }), 'success', 5000)
    } catch (e) {
      // 会话关闭导致的取消：静默（进度条已在 cancelSessionTransfers 移除）
      if (cancelledTransfers.has(taskId)) return
      showToast(t('toast.downloadFailed', { err: String(e) }), 'error', 5000)
    } finally {
      downloading.value[remotePath] = false
      // 校验完成（成功/失败/取消）：移除"校验中"toast（随后结果 toast 同位置衔接）
      const vt = verifyingToasts[taskId]
      if (vt !== undefined) {
        removeToast(vt)
        delete verifyingToasts[taskId]
      }
      if (cancelledTransfers.has(taskId)) {
        cancelledTransfers.delete(taskId)
        delete transfers.value[taskId]
        return
      }
      // 完成后短暂保留进度条（显示 100%），随后移除
      setTimeout(() => { delete transfers.value[taskId] }, 1500)
    }
  }

  async function uploadFile(sessionId: string, remoteDir: string, localPath: string, expectedDir?: string, isDir = false) {
    if (!sessionId) return
    // 防重入：同一文件/目录正在上传时忽略重复操作
    if (uploading.value[localPath]) {
      showToast(t('toast.uploadInProgress'), 'info')
      return
    }
    uploading.value[localPath] = true
    const fileName = localPath.split('\\').pop() || 'upload'
    const remotePath = remoteDir.replace(/\/$/, '') + '/' + fileName

    // 创建传输任务
    const taskId = crypto.randomUUID()
    transfers.value[taskId] = { id: taskId, sessionId, name: fileName, direction: 'upload', done: 0, total: 0 }
    try {
      // 目录走递归传输命令；单文件保留 SHA-256 校验
      if (isDir) {
        await invoke('sftp_upload_tree', {
          sessionId, remotePath, localPath, taskId,
          expectedDir: expectedDir || homeDir.value || '',
        })
      } else {
        await invoke('sftp_upload_file', {
          sessionId, remotePath, localPath, taskId,
          expectedDir: expectedDir || homeDir.value || '',
        })
      }
      if (cancelledTransfers.has(taskId)) return // 会话已关闭：迟到结果静默
      // 刷新对应标签的远程文件树
      bumpRemoteRefresh(sessionId)
      showToast(t('toast.uploaded', { path: remotePath }), 'success', 5000)
    } catch (e) {
      // 会话关闭导致的取消：静默
      if (cancelledTransfers.has(taskId)) return
      showToast(t('toast.uploadFailed', { err: String(e) }), 'error', 5000)
    } finally {
      uploading.value[localPath] = false
      // 校验完成（成功/失败/取消）：移除"校验中"toast
      const vt = verifyingToasts[taskId]
      if (vt !== undefined) {
        removeToast(vt)
        delete verifyingToasts[taskId]
      }
      if (cancelledTransfers.has(taskId)) {
        cancelledTransfers.delete(taskId)
        delete transfers.value[taskId]
        return
      }
      setTimeout(() => { delete transfers.value[taskId] }, 1500)
    }
  }

  return {
    transfers, downloading, uploading,
    localRefresh, remoteRefresh,
    homeDir,
    downloadFile, uploadFile, downloadMany, uploadMany,
    cancelSessionTransfers, handleTransferProgress,
  }
}
