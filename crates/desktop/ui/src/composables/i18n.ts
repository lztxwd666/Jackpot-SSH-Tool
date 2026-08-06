// i18n 模块
// 默认语言：英文（en），支持中文（zh）
// 设计说明：
//   - currentLocale 是 Vue ref：模板中调用 t() 时会建立响应式依赖，
//     切换语言后所有组件自动重新渲染
//   - 语言选择持久化到 localStorage，重启后保持
//   - 新增语言：在 messages 中添加对应 locale 的条目即可
// 规则：所有用户可见的界面字符串必须通过 t() 引用，禁止在组件中硬编码

import { ref } from 'vue'

export type Locale = 'en' | 'zh'

export const locales: Locale[] = ['en', 'zh']

const STORAGE_KEY = 'jackpot-locale'

function loadStoredLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'en' || saved === 'zh') return saved
  } catch (_) { }
  return 'en'
}

const messages: Record<Locale, Record<string, string>> = {
  en: {
    // 通用按钮
    'common.ok': 'OK',
    'common.cancel': 'Cancel',
    'common.confirm': 'Confirm',
    'common.input': 'Input',
    'common.loading': 'Loading...',
    'common.refresh': 'Refresh',
    'common.connecting': 'Connecting...',
    'common.connect': 'Connect',
    'common.edit': 'Edit',
    'common.disconnect': 'Disconnect',
    'common.save': 'Save',
    'common.delete': 'Delete',
    'common.rename': 'Rename',
    'common.ping': 'Ping',
    'common.newFolder': 'New Folder',
    'common.download': 'Download',
    'common.upload': 'Upload to Remote',

    // 主机列表
    'hosts.title': 'Hosts',
    'hosts.add': '+ Add',
    'hosts.searchPlaceholder': 'Search hosts...',
    'hosts.selectHint': 'Select a host from the list or add a new one.',
    'hosts.deleteConfirm': 'Delete "{name}"?',
    'hosts.enterPassword': 'Enter Password',
    'hosts.connectingTo': 'Connecting to {user}@{host}',
    'hosts.passwordPlaceholder': 'Password',
    'hosts.newHost': 'New Host',
    'hosts.editHost': 'Edit Host',
    'hosts.savePasswordOnConnect': 'Save this password',

    // 主机表单
    'form.name': 'Name',
    'form.namePlaceholder': 'My Server',
    'form.address': 'Address',
    'form.addressPlaceholder': '192.168.1.1',
    'form.port': 'Port',
    'form.username': 'Username',
    'form.usernamePlaceholder': 'root',
    'form.authType': 'Auth Type',
    'form.authPassword': 'Password',
    'form.authPrivateKey': 'Private Key',
    'form.authAgent': 'SSH Agent',
    'form.group': 'Group',
    'form.groupNone': 'No group',
    'form.groupNew': 'New group...',
    'form.groupNewName': 'New group name',
    'form.groupPlaceholder': 'Production',
    'form.notes': 'Notes',
    'form.notesPlaceholder': 'Optional notes...',
    'form.favorite': 'Favorite',
    'form.password': 'Password',
    'form.passwordPlaceholder': 'Enter password',
    'form.passphrase': 'Passphrase',
    'form.showSecret': 'Show',
    'form.hideSecret': 'Hide',
    'form.savePassword': 'Save password',
    'form.savePassphrase': 'Save passphrase',
    'form.passwordSaved': 'Password saved (enter new to change)',

    // 主机密钥确认
    'hostkey.confirmTitle': 'Confirm Host Key',
    'hostkey.changedTitle': 'Warning: Host Key Changed',
    'hostkey.unknown': "The authenticity of host '{host}' can't be established.\nFingerprint: {fp}\n\nTrust this host and continue connecting?",
    'hostkey.changed': "Host key CHANGED for {host}!\nOld: {old}\nNew: {new}\n\nThis may indicate a man-in-the-middle attack. Trust the new key?",
    'hostkey.saveFailed': 'Failed to save host key: {err}',

    // 主机详情
    'detail.address': 'Address',
    'detail.username': 'Username',
    'detail.auth': 'Auth',
    'detail.group': 'Group',
    'detail.notes': 'Notes',

    // 文件树
    'tree.localTitle': 'Local Files',
    'tree.remoteTitle': 'Remote Files',
    'hosts.groupUnassigned': 'Unassigned',
    'tree.transferLocked': 'Transfer in progress — file tree temporarily locked',

    // 输入对话框
    'prompt.folderName': 'Folder name:',
    'prompt.newName': 'New name:',

    // 标签页
    'tab.disconnect': 'Disconnect',
    'tab.close': 'Close',
    'tab.connecting': 'Connecting...',
    'tab.disconnected': 'Disconnected: {reason}',
    'tab.reconnecting': 'Reconnecting...',
    'tab.reconnect': 'Reconnect',
    'tab.overlayDisconnected': 'Connection lost',
    'tab.transferBusy': 'Transfer in progress — bandwidth occupied, commands may be delayed',

    // 状态栏
    'status.running': 'Running',
    'status.stopped': 'Stopped',
    'status.unknown': 'Unknown',
    'status.initializing': 'Initializing...',

    // Toast 提示
    'toast.downloadInProgress': 'Download already in progress',
    'toast.downloaded': 'Downloaded to {path}',
    'toast.downloadFailed': 'Download failed: {err}',
    'toast.uploadInProgress': 'Upload already in progress',
    'toast.uploaded': 'Uploaded to {path}',
    'toast.uploadFailed': 'Upload failed: {err}',
    'toast.connectionFailed': 'Connection failed: {err}',
    'toast.connectTimeout': 'Connection timed out',
    'toast.pingOk': 'Ping OK, {ms}ms',
    'toast.pingOkNoLatency': 'Ping OK',
    'toast.pingFail': 'Ping failed',
    'toast.created': 'Created {name}',
    'toast.renamed': 'Renamed to {name}',
    'toast.deleted': 'Deleted {name}',
    'toast.createFailed': 'Create folder failed: {err}',
    'toast.renameFailed': 'Rename failed: {err}',
    'toast.deleteFailed': 'Delete failed: {err}',
    'toast.notConnected': 'Not connected to any host',
    'toast.systemFileDrop': 'Please use the local file tree to upload files',
    'toast.hostNotFound': 'Host not found',
  },
  zh: {
    // 通用按钮
    'common.ok': '确定',
    'common.cancel': '取消',
    'common.confirm': '确认',
    'common.input': '输入',
    'common.loading': '加载中...',
    'common.refresh': '刷新',
    'common.connecting': '连接中...',
    'common.connect': '连接',
    'common.edit': '编辑',
    'common.disconnect': '断开连接',
    'common.save': '保存',
    'common.delete': '删除',
    'common.rename': '重命名',
    'common.ping': 'Ping',
    'common.newFolder': '新建文件夹',
    'common.download': '下载',
    'common.upload': '上传到远程',

    // 主机列表
    'hosts.title': '主机',
    'hosts.add': '+ 添加',
    'hosts.searchPlaceholder': '搜索主机...',
    'hosts.selectHint': '请从列表中选择主机或添加新主机。',
    'hosts.deleteConfirm': '确定删除 "{name}"？',
    'hosts.enterPassword': '输入密码',
    'hosts.connectingTo': '正在连接到 {user}@{host}',
    'hosts.passwordPlaceholder': '密码',
    'hosts.newHost': '新建主机',
    'hosts.editHost': '编辑主机',
    'hosts.savePasswordOnConnect': '保存此密码',

    // 主机表单
    'form.name': '名称',
    'form.namePlaceholder': '我的服务器',
    'form.address': '地址',
    'form.addressPlaceholder': '192.168.1.1',
    'form.port': '端口',
    'form.username': '用户名',
    'form.usernamePlaceholder': 'root',
    'form.authType': '认证方式',
    'form.authPassword': '密码',
    'form.authPrivateKey': '私钥',
    'form.authAgent': 'SSH Agent',
    'form.group': '分组',
    'form.groupNone': '未分组',
    'form.groupNew': '新建分组...',
    'form.groupNewName': '新分组名称',
    'form.groupPlaceholder': '生产环境',
    'form.notes': '备注',
    'form.notesPlaceholder': '可选备注...',
    'form.favorite': '收藏',
    'form.password': '密码',
    'form.passwordPlaceholder': '输入密码',
    'form.passphrase': '口令',
    'form.showSecret': '显示',
    'form.hideSecret': '隐藏',
    'form.savePassword': '保存密码',
    'form.savePassphrase': '保存口令',
    'form.passwordSaved': '密码已保存（更改请输入新密码）',

    // 主机密钥确认
    'hostkey.confirmTitle': '确认主机密钥',
    'hostkey.changedTitle': '警告：主机密钥已变更',
    'hostkey.unknown': "无法确认主机 '{host}' 的真实性。\n指纹：{fp}\n\n是否信任此主机并继续连接？",
    'hostkey.changed': "主机 {host} 的密钥已变更！\n旧指纹：{old}\n新指纹：{new}\n\n这可能表示存在中间人攻击。是否信任新密钥？",
    'hostkey.saveFailed': '保存主机密钥失败：{err}',

    // 主机详情
    'detail.address': '地址',
    'detail.username': '用户名',
    'detail.auth': '认证',
    'detail.group': '分组',
    'detail.notes': '备注',

    // 文件树
    'tree.localTitle': '本地文件',
    'tree.remoteTitle': '远程文件',
    'hosts.groupUnassigned': '未分组',
    'tree.transferLocked': '传输进行中，文件树暂时锁定',

    // 输入对话框
    'prompt.folderName': '文件夹名称：',
    'prompt.newName': '新名称：',

    // 标签页
    'tab.disconnect': '断开连接',
    'tab.close': '关闭',
    'tab.connecting': '正在连接...',
    'tab.disconnected': '连接已断开：{reason}',
    'tab.reconnecting': '正在重连...',
    'tab.reconnect': '重连',
    'tab.overlayDisconnected': '连接已断开',
    'tab.transferBusy': '传输进行中，带宽被占满，命令可能延后',

    // 状态栏
    'status.running': '运行中',
    'status.stopped': '已停止',
    'status.unknown': '未知',
    'status.initializing': '初始化中...',

    // Toast 提示
    'toast.downloadInProgress': '该文件正在下载中',
    'toast.downloaded': '已下载到 {path}',
    'toast.downloadFailed': '下载失败: {err}',
    'toast.uploadInProgress': '该文件正在上传中',
    'toast.uploaded': '已上传到 {path}',
    'toast.uploadFailed': '上传失败: {err}',
    'toast.connectionFailed': '连接失败: {err}',
    'toast.connectTimeout': '连接超时',
    'toast.pingOk': 'Ping 成功，{ms} 毫秒',
    'toast.pingOkNoLatency': 'Ping 成功',
    'toast.pingFail': 'Ping 失败',
    'toast.created': '已创建 {name}',
    'toast.renamed': '已重命名为 {name}',
    'toast.deleted': '已删除 {name}',
    'toast.createFailed': '创建文件夹失败: {err}',
    'toast.renameFailed': '重命名失败: {err}',
    'toast.deleteFailed': '删除失败: {err}',
    'toast.notConnected': '尚未连接到任何主机',
    'toast.systemFileDrop': '请使用左侧本地文件树上传文件',
    'toast.hostNotFound': '主机不存在',
  },
}

// 响应式当前语言：模板中调用 t() 时会追踪此 ref，切换后自动重渲染
const currentLocale = ref<Locale>(loadStoredLocale())

/** 切换语言并持久化 */
export function setLocale(locale: Locale) {
  currentLocale.value = locale
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch (_) { }
}

/** 获取当前语言 */
export function getLocale(): Locale {
  return currentLocale.value
}

/** 翻译：t('toast.downloaded', { path: 'C:\\x\\a.txt' }) */
export function t(key: string, vars?: Record<string, string>): string {
  const table = messages[currentLocale.value] ?? messages.en
  let s = table[key] ?? messages.en[key] ?? key
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.split(`{${k}}`).join(v)
    }
  }
  return s
}
