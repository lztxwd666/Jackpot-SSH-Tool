<script setup lang="ts">
// 主机新增/编辑滑出面板：从右侧覆盖主机栏（不覆盖工作区），带滑出动画
// 覆盖式交互：完成/取消后才可进行下一次操作（主机栏无并发需求）
import { computed, ref, watch } from 'vue'
import { t } from '../composables/i18n'

export interface HostForm {
  id?: string
  name: string
  address: string
  port: number
  username: string
  auth_type: string
  group_name: string
  favorite: boolean
  notes: string
  // 凭据相关（Task B4 完整接线密码框/保存勾选；本任务先建字段骨架）
  password: string
  save_password: boolean
}

const props = defineProps<{ open: boolean; mode: 'new' | 'edit'; initial: HostForm | null; groups: string[] }>()
const emit = defineEmits<{ (e: 'save', form: HostForm): void; (e: 'cancel'): void }>()

function emptyForm(): HostForm {
  return {
    name: '', address: '', port: 22, username: 'root', auth_type: 'password',
    group_name: '', favorite: false, notes: '', password: '', save_password: false,
  }
}

const form = ref<HostForm>(emptyForm())

// 密码查看按钮：小眼睛图标式（密文/明文切换，标准设计）
const showSecret = ref(false)

// 编辑已保存密码的主机：密码框占位提示"密码已保存（更改请输入新密码）"，不显示明文
const secretPlaceholder = computed(() =>
  props.mode === 'edit' && form.value.save_password ? t('form.passwordSaved') : t('form.passwordPlaceholder'))

// 分组下拉：选择已有分组 / 新建分组（新建时显示输入框）
// groupSelect 为下拉当前值（'' 未分组 | 分组名 | '__new__' 新建）；form.group_name 为最终保存值
const groupSelect = ref('')
const creatingGroup = ref(false)
const existingGroups = computed(() => props.groups.filter(g => g && g !== ''))
watch(groupSelect, (v) => {
  if (v === '__new__') {
    creatingGroup.value = true
    form.value.group_name = ''
  } else {
    creatingGroup.value = false
    form.value.group_name = v
  }
})

// 打开时按模式装载初始值；编辑模式不回显密码（保留 save_password 勾选状态）
watch(() => props.open, (v) => {
  if (v) {
    form.value = props.initial
      ? { ...props.initial, password: '' }
      : emptyForm()
    showSecret.value = false
    creatingGroup.value = false
    groupSelect.value = form.value.group_name || ''
  }
})

function submit() {
  emit('save', { ...form.value })
  // 安全约定：密码只经 IPC 参数传递，提交后前端立即清空
  form.value.password = ''
}
</script>

<template>
  <div class="form-overlay" :class="{ open }">
    <div class="form-panel">
      <div class="panel-header">
        <h3>{{ mode === 'new' ? t('hosts.newHost') : t('hosts.editHost') }}</h3>
        <button class="btn" @click="emit('cancel')">{{ t('common.cancel') }}</button>
      </div>
      <form class="host-form" @submit.prevent="submit">
        <label>{{ t('form.name') }} <input v-model="form.name" type="text" required :placeholder="t('form.namePlaceholder')" /></label>
        <label>{{ t('form.address') }} <input v-model="form.address" type="text" required :placeholder="t('form.addressPlaceholder')" /></label>
        <label>{{ t('form.port') }} <input v-model.number="form.port" type="number" required min="1" max="65535" /></label>
        <label>{{ t('form.username') }} <input v-model="form.username" type="text" required :placeholder="t('form.usernamePlaceholder')" /></label>
        <label>{{ t('form.authType') }}
          <select v-model="form.auth_type">
            <option value="password">{{ t('form.authPassword') }}</option>
            <option value="private_key">{{ t('form.authPrivateKey') }}</option>
            <option value="agent">{{ t('form.authAgent') }}</option>
          </select>
        </label>
        <!-- 认证相关字段聚合：密码/口令框紧随认证方式（用户反馈：置于表单底部不合理） -->
        <!-- 密码认证：密码框 + 小眼睛查看图标 + 保存勾选 -->
        <div v-if="form.auth_type === 'password'" class="secret-field">
          <label>{{ t('form.password') }}
            <div class="secret-row">
              <input v-model="form.password" :type="showSecret ? 'text' : 'password'" :placeholder="secretPlaceholder" autocomplete="new-password" />
              <!-- 小眼睛图标：查看/隐藏密码切换（feather eye 风格，替代文字按钮） -->
              <button type="button" class="eye-toggle" :title="showSecret ? t('form.hideSecret') : t('form.showSecret')" @click="showSecret = !showSecret">
                <svg v-if="!showSecret" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
                </svg>
                <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>
                </svg>
              </button>
            </div>
          </label>
          <label class="checkbox-label">
            <input v-model="form.save_password" type="checkbox" /> {{ t('form.savePassword') }}
          </label>
        </div>
        <!-- 私钥认证：口令框 + 小眼睛查看图标 + 保存口令勾选（私钥路径字段待后续任务接线） -->
        <div v-else-if="form.auth_type === 'private_key'" class="secret-field">
          <label>{{ t('form.passphrase') }}
            <div class="secret-row">
              <input v-model="form.password" :type="showSecret ? 'text' : 'password'" :placeholder="secretPlaceholder" autocomplete="new-password" />
              <button type="button" class="eye-toggle" :title="showSecret ? t('form.hideSecret') : t('form.showSecret')" @click="showSecret = !showSecret">
                <svg v-if="!showSecret" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
                </svg>
                <svg v-else width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>
                </svg>
              </button>
            </div>
          </label>
          <label class="checkbox-label">
            <input v-model="form.save_password" type="checkbox" /> {{ t('form.savePassphrase') }}
          </label>
        </div>
        <!-- 分组下拉：选择已有分组 / 新建分组（常见程序设计逻辑） -->
        <label>{{ t('form.group') }}
          <select v-model="groupSelect">
            <option value="">{{ t('form.groupNone') }}</option>
            <option v-for="g in existingGroups" :key="g" :value="g">{{ g }}</option>
            <option value="__new__">{{ t('form.groupNew') }}</option>
          </select>
          <input v-if="creatingGroup" v-model="form.group_name" type="text" :placeholder="t('form.groupNewName')" />
        </label>
        <label>{{ t('form.notes') }} <textarea v-model="form.notes" rows="3" :placeholder="t('form.notesPlaceholder')"></textarea></label>
        <label class="checkbox-label"><input v-model="form.favorite" type="checkbox" /> {{ t('form.favorite') }}</label>
        <div class="form-actions">
          <button type="submit" class="btn btn-primary">{{ t('common.save') }}</button>
        </div>
      </form>
    </div>
  </div>
</template>

<style scoped>
/* position: fixed 覆盖右侧主机栏；transform: translateX 滑出动画（约 200ms ease） */
/* 宽度与右侧主机栏一致（220px），视觉不突兀（用户反馈） */
.form-overlay { position: fixed; top: 0; right: 0; bottom: 0; width: 220px; background: var(--color-background); border-left: 1px solid var(--color-border); box-shadow: -4px 0 16px rgba(0,0,0,0.2); transform: translateX(100%); transition: transform 0.2s ease; z-index: 900; }
.form-overlay.open { transform: translateX(0); }
.form-panel { display: flex; flex-direction: column; height: 100%; padding: 1rem; overflow-y: auto; }
.panel-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
.panel-header h3 { color: var(--color-heading); font-size: 0.95rem; }
.host-form { display: flex; flex-direction: column; gap: 0.8rem; }
.host-form label { display: flex; flex-direction: column; gap: 0.2rem; font-size: 0.8rem; color: var(--color-text); }
.host-form input, .host-form select, .host-form textarea {
  padding: 0.4rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); font-size: 0.85rem; font-family: inherit;
}
.host-form textarea { resize: vertical; }
.secret-field { display: flex; flex-direction: column; gap: 0.4rem; }
.secret-row { display: flex; gap: 0.3rem; }
.secret-row input { flex: 1; min-width: 0; }
/* 小眼睛图标按钮：无边框透明底，悬停显背景（密码查看切换） */
.eye-toggle {
  display: inline-flex; align-items: center; justify-content: center;
  padding: 0.3rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer;
}
.eye-toggle:hover { background: var(--color-background-mute); }
.checkbox-label { flex-direction: row !important; align-items: center; gap: 0.4rem !important; }
.form-actions { margin-top: 0.4rem; }

.btn {
  padding: 0.3rem 0.7rem; border: 1px solid var(--color-border); border-radius: 4px;
  background: var(--color-background); color: var(--color-text); cursor: pointer; font-size: 0.8rem;
}
.btn:hover { background: var(--color-background-mute); }
.btn-primary { background: hsla(160, 100%, 37%, 1); color: #fff; border-color: hsla(160, 100%, 37%, 1); }
.btn-primary:hover { background: hsla(160, 100%, 30%, 1); }
</style>
