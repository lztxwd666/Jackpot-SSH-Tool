<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import Terminal from './components/Terminal.vue'

interface Host {
  id: string
  name: string
  address: string
  port: number
  username: string
  auth_type: string
  group_name: string
  favorite: boolean
  notes: string
  created_at: string
  updated_at: string
}

const hosts = ref<Host[]>([])
const searchQuery = ref('')
const status = ref('initializing...')

const editing = ref(false)
const selectedHost = ref<Host | null>(null)
const form = ref({
  name: '',
  address: '',
  port: 22,
  username: 'root',
  auth_type: 'password',
  group_name: '',
  favorite: false,
  notes: '',
})

const connecting = ref(false)
const connected = ref(false)
const channelId = ref('')
const sessionId = ref('')
const password = ref('')
const showPasswordPrompt = ref(false)

async function loadHosts() {
  try {
    hosts.value = await invoke('list_hosts')
  } catch (e) {
    console.error('Failed to load hosts:', e)
  }
}

async function doSearch() {
  const q = searchQuery.value.trim()
  if (!q) {
    await loadHosts()
    return
  }
  try {
    hosts.value = await invoke('search_hosts', { query: q })
  } catch (e) {
    console.error('Search failed:', e)
  }
}

function newHost() {
  editing.value = true
  selectedHost.value = null
  form.value = {
    name: '',
    address: '',
    port: 22,
    username: 'root',
    auth_type: 'password',
    group_name: '',
    favorite: false,
    notes: '',
  }
}

function editHost(host: Host) {
  editing.value = true
  selectedHost.value = host
  form.value = {
    name: host.name,
    address: host.address,
    port: host.port,
    username: host.username,
    auth_type: host.auth_type,
    group_name: host.group_name,
    favorite: host.favorite,
    notes: host.notes,
  }
}

async function saveHost() {
  try {
    const id = selectedHost.value ? selectedHost.value.id : crypto.randomUUID()
    await invoke('save_host', {
      host: {
        id,
        ...form.value,
        created_at: selectedHost.value?.created_at ?? '',
        updated_at: new Date().toISOString(),
      },
    })
    editing.value = false
    selectedHost.value = null
    await loadHosts()
  } catch (e) {
    console.error('Save failed:', e)
  }
}

async function deleteHost(host: Host) {
  if (!confirm(`Delete "${host.name}"?`)) return
  try {
    await invoke('delete_host', { id: host.id })
    if (selectedHost.value?.id === host.id) {
      editing.value = false
      selectedHost.value = null
      connecting.value = false
    }
    await loadHosts()
  } catch (e) {
    console.error('Delete failed:', e)
  }
}

function cancelEdit() {
  editing.value = false
  selectedHost.value = null
}

function selectHost(host: Host) {
  selectedHost.value = host
  editing.value = false
  connected.value = false
  connecting.value = false
  channelId.value = ''
}

function promptConnect() {
  if (!selectedHost.value) return
  if (selectedHost.value.auth_type === 'password') {
    showPasswordPrompt.value = true
    password.value = ''
  }
}

async function doConnect() {
  if (!selectedHost.value) return

  // 先断开旧连接
  if (sessionId.value) {
    try {
      await invoke('terminal_close', { sessionId: sessionId.value })
    } catch (_) {}
    connected.value = false
    channelId.value = ''
    sessionId.value = ''
  }

  connecting.value = true
  showPasswordPrompt.value = false

  try {
    const sid = await invoke('create_session') as string
    sessionId.value = sid

    await invoke('connect_session', {
      sessionId: sid,
      host: selectedHost.value.address,
      port: selectedHost.value.port,
      username: selectedHost.value.username,
      password: password.value,
    })

    const cid = await invoke('open_shell', { sessionId: sid }) as string
    channelId.value = cid
    connected.value = true
  } catch (e) {
    console.error('Connect failed:', e)
    alert(`Connection failed: ${e}`)
  } finally {
    connecting.value = false
  }
}

function cancelConnect() {
  showPasswordPrompt.value = false
  password.value = ''
  connecting.value = false
}

async function disconnect() {
  if (sessionId.value) {
    try {
      await invoke('terminal_close', { sessionId: sessionId.value })
    } catch (_) {}
  }
  connected.value = false
  channelId.value = ''
  sessionId.value = ''
}

onMounted(async () => {
  status.value = 'running'
  await loadHosts()
  await listen<string>('core-event', (event) => {
    const parsed = JSON.parse(event.payload)
    if (parsed.type === 'Host') {
      loadHosts()
    }
  })
})
</script>

<template>
  <div class="app-layout">
    <aside class="sidebar">
      <div class="sidebar-header">
        <h2>Hosts</h2>
        <button class="btn btn-primary" @click="newHost">+ Add</button>
      </div>
      <div class="search-bar">
        <input
          v-model="searchQuery"
          type="text"
          placeholder="Search hosts..."
          @input="doSearch"
        />
      </div>
      <ul class="host-list">
        <li
          v-for="host in hosts"
          :key="host.id"
          :class="{ active: selectedHost?.id === host.id }"
          @click="selectHost(host)"
        >
          <span class="host-name">{{ host.name }}</span>
          <span class="host-addr">{{ host.address }}:{{ host.port }}</span>
        </li>
      </ul>
      <div class="sidebar-footer">
        <span class="status-badge">{{ status }}</span>
      </div>
    </aside>

    <main class="main-panel">
      <template v-if="connected && channelId">
        <div class="terminal-wrapper">
          <div class="terminal-header">
            <span class="connection-info">
              {{ selectedHost?.name }} ({{ selectedHost?.username }}@{{ selectedHost?.address }}:{{ selectedHost?.port }})
            </span>
            <button class="btn btn-danger" @click="disconnect">Disconnect</button>
          </div>
          <Terminal :channelId="channelId" :key="channelId" />
        </div>
      </template>
      <template v-else-if="editing">
        <div class="form-header">
          <h3>{{ selectedHost ? 'Edit Host' : 'New Host' }}</h3>
        </div>
        <form class="host-form" @submit.prevent="saveHost">
          <label>
            Name
            <input v-model="form.name" type="text" required placeholder="My Server" />
          </label>
          <label>
            Address
            <input v-model="form.address" type="text" required placeholder="192.168.1.1" />
          </label>
          <label>
            Port
            <input v-model.number="form.port" type="number" required min="1" max="65535" />
          </label>
          <label>
            Username
            <input v-model="form.username" type="text" required placeholder="root" />
          </label>
          <label>
            Auth Type
            <select v-model="form.auth_type">
              <option value="password">Password</option>
              <option value="private_key">Private Key</option>
              <option value="agent">SSH Agent</option>
            </select>
          </label>
          <label>
            Group
            <input v-model="form.group_name" type="text" placeholder="Production" />
          </label>
          <label>
            Notes
            <textarea v-model="form.notes" rows="3" placeholder="Optional notes..."></textarea>
          </label>
          <label class="checkbox-label">
            <input v-model="form.favorite" type="checkbox" />
            Favorite
          </label>
          <div class="form-actions">
            <button type="submit" class="btn btn-primary">Save</button>
            <button type="button" class="btn" @click="cancelEdit">Cancel</button>
            <button
              v-if="selectedHost"
              type="button"
              class="btn btn-danger"
              @click="deleteHost(selectedHost!)"
            >
              Delete
            </button>
          </div>
        </form>
      </template>
      <template v-else-if="selectedHost">
        <div class="host-detail">
          <div class="host-detail-header">
            <h3>{{ selectedHost.name }}</h3>
            <div class="host-detail-actions">
              <button class="btn btn-primary" @click="promptConnect" :disabled="connecting">
                {{ connecting ? 'Connecting...' : 'Connect' }}
              </button>
              <button class="btn" @click="editHost(selectedHost!)">Edit</button>
            </div>
          </div>
          <div class="host-detail-info">
            <div class="info-row">
              <span class="info-label">Address</span>
              <span class="info-value">{{ selectedHost.address }}:{{ selectedHost.port }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">Username</span>
              <span class="info-value">{{ selectedHost.username }}</span>
            </div>
            <div class="info-row">
              <span class="info-label">Auth</span>
              <span class="info-value">{{ selectedHost.auth_type }}</span>
            </div>
            <div class="info-row" v-if="selectedHost.group_name">
              <span class="info-label">Group</span>
              <span class="info-value">{{ selectedHost.group_name }}</span>
            </div>
            <div class="info-row" v-if="selectedHost.notes">
              <span class="info-label">Notes</span>
              <span class="info-value">{{ selectedHost.notes }}</span>
            </div>
          </div>
        </div>
      </template>
      <template v-else>
        <div class="placeholder">
          <p>Select a host from the list or add a new one.</p>
        </div>
      </template>

      <div v-if="showPasswordPrompt" class="modal-overlay" @click.self="cancelConnect">
        <div class="modal">
          <h3>Enter Password</h3>
          <p>Connecting to {{ selectedHost?.username }}@{{ selectedHost?.address }}</p>
          <form @submit.prevent="doConnect">
            <input
              v-model="password"
              type="password"
              placeholder="Password"
              autofocus
              required
            />
            <div class="modal-actions">
              <button type="submit" class="btn btn-primary" :disabled="connecting">
                {{ connecting ? 'Connecting...' : 'Connect' }}
              </button>
              <button type="button" class="btn" @click="cancelConnect">Cancel</button>
            </div>
          </form>
        </div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  height: 100vh;
  width: 100vw;
  overflow: hidden;
}

.sidebar {
  width: 280px;
  min-width: 280px;
  background: var(--color-background-soft);
  border-right: 1px solid var(--color-border);
  display: flex;
  flex-direction: column;
}

.sidebar-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem;
  border-bottom: 1px solid var(--color-border);
}

.sidebar-header h2 {
  font-size: 1.1rem;
  color: var(--color-heading);
}

.search-bar {
  padding: 0.5rem 1rem;
  border-bottom: 1px solid var(--color-border);
}

.search-bar input {
  width: 100%;
  padding: 0.4rem 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 0.85rem;
}

.host-list {
  flex: 1;
  overflow-y: auto;
  list-style: none;
  padding: 0;
  margin: 0;
}

.host-list li {
  padding: 0.6rem 1rem;
  cursor: pointer;
  border-bottom: 1px solid var(--color-border);
  transition: background 0.15s;
}

.host-list li:hover {
  background: var(--color-background-mute);
}

.host-list li.active {
  background: var(--color-border-hover);
}

.host-name {
  display: block;
  font-weight: 600;
  color: var(--color-heading);
  font-size: 0.9rem;
}

.host-addr {
  font-size: 0.75rem;
  color: var(--color-text);
  opacity: 0.7;
}

.sidebar-footer {
  padding: 0.5rem 1rem;
  border-top: 1px solid var(--color-border);
}

.status-badge {
  font-size: 0.75rem;
  color: hsla(160, 100%, 37%, 1);
}

.main-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.form-header h3 {
  margin-bottom: 1.5rem;
  color: var(--color-heading);
}

.host-form {
  max-width: 480px;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 2rem;
}

.host-form label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.85rem;
  color: var(--color-text);
}

.host-form input,
.host-form select,
.host-form textarea {
  padding: 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 0.9rem;
  font-family: inherit;
}

.host-form textarea {
  resize: vertical;
}

.checkbox-label {
  flex-direction: row !important;
  align-items: center;
  gap: 0.5rem !important;
}

.form-actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 0.5rem;
}

.btn {
  padding: 0.4rem 0.8rem;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  font-size: 0.85rem;
  transition: background 0.15s;
}

.btn:hover {
  background: var(--color-background-mute);
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.btn-primary {
  background: hsla(160, 100%, 37%, 1);
  color: #fff;
  border-color: hsla(160, 100%, 37%, 1);
}

.btn-primary:hover {
  background: hsla(160, 100%, 30%, 1);
}

.btn-danger {
  color: #e5534b;
  border-color: #e5534b;
}

.btn-danger:hover {
  background: #e5534b;
  color: #fff;
}

.placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--color-text);
  opacity: 0.5;
}

.host-detail {
  padding: 2rem;
}

.host-detail-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.5rem;
}

.host-detail-header h3 {
  color: var(--color-heading);
  font-size: 1.3rem;
}

.host-detail-actions {
  display: flex;
  gap: 0.5rem;
}

.host-detail-info {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.info-row {
  display: flex;
  gap: 1rem;
  align-items: baseline;
}

.info-label {
  font-size: 0.8rem;
  color: var(--color-text);
  opacity: 0.6;
  min-width: 80px;
}

.info-value {
  font-size: 0.9rem;
  color: var(--color-text);
}

.terminal-wrapper {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.terminal-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.5rem 1rem;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
  position: relative;
  z-index: 10;
  flex-shrink: 0;
}

.connection-info {
  font-size: 0.85rem;
  color: var(--color-text);
  font-weight: 500;
}

.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.modal {
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 8px;
  padding: 1.5rem;
  min-width: 320px;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.3);
}

.modal h3 {
  color: var(--color-heading);
  margin-bottom: 0.5rem;
}

.modal p {
  color: var(--color-text);
  opacity: 0.7;
  font-size: 0.85rem;
  margin-bottom: 1rem;
}

.modal input {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background-soft);
  color: var(--color-text);
  font-size: 0.9rem;
  margin-bottom: 1rem;
  box-sizing: border-box;
}

.modal-actions {
  display: flex;
  gap: 0.5rem;
  justify-content: flex-end;
}
</style>
