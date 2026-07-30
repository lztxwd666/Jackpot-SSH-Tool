<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { listen } from '@tauri-apps/api/event'

const status = ref('initializing...')
const events = ref<string[]>([])

onMounted(async () => {
  status.value = 'running'
  await listen<string>('core-event', (event) => {
    const parsed = JSON.parse(event.payload)
    events.value.unshift(`${new Date().toLocaleTimeString()}: ${parsed.type}`)
  })
})
</script>

<template>
  <div class="container">
    <h1>Jackpot SSH Tool</h1>
    <p>Status: {{ status }}</p>
    <div class="events">
      <h3>Events</h3>
      <ul>
        <li v-for="(e, i) in events" :key="i">{{ e }}</li>
      </ul>
    </div>
  </div>
</template>

<style>
.container { padding: 2rem; font-family: monospace; }
.events { margin-top: 1rem; }
.events ul { list-style: none; padding: 0; }
.events li { padding: 0.25rem 0; border-bottom: 1px solid #eee; }
</style>
