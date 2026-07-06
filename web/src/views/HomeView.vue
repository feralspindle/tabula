<script setup>
import { ref, onMounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { apiClient } from '@/lib/apiClient'
import { useAuthStore } from '@/stores/authStore'
import LoginPanel from '@/components/LoginPanel.vue'

const auth = useAuthStore()
const router = useRouter()

const sessions = ref([])
const plugins = ref([])
const name = ref('')
const pluginId = ref('')
const joinId = ref('')
const error = ref(null)
const busy = ref(false)

async function load() {
  if (!auth.isAuthenticated) return
  try {
    ;[sessions.value, plugins.value] = await Promise.all([
      apiClient.get('/sessions'),
      apiClient.get('/plugins'),
    ])
    const systems = plugins.value.filter((p) => p.plugin_type === 'system')
    if (!pluginId.value && systems.length) pluginId.value = systems[0].id
  } catch (e) {
    error.value = e.message
  }
}

onMounted(load)
watch(() => auth.isAuthenticated, load)

async function createSession() {
  error.value = null
  busy.value = true
  try {
    const session = await apiClient.post(
      '/sessions',
      { name: name.value, system_plugin_id: pluginId.value },
      'create_session',
    )
    router.push({ name: 'session', params: { id: session.id } })
  } catch (e) {
    error.value = e.message
  } finally {
    busy.value = false
  }
}

function join() {
  const id = joinId.value.trim().split('/').pop()
  if (id) router.push({ name: 'session', params: { id } })
}
</script>

<template>
  <div class="max-w-6xl mx-auto px-4 py-10">
    <div v-if="auth.loading" class="text-center text-bone-500 py-20 font-ledger text-sm">…</div>

    <template v-else-if="!auth.isAuthenticated">
      <div class="text-center mb-10 leaf">
        <h1 class="font-display text-5xl text-bone-100 mb-3">The table is a ledger.</h1>
        <p class="text-bone-500 max-w-xl mx-auto">
          Tabula is a system-agnostic virtual tabletop. Game rules live in sandboxed plugins;
          every change at the table is one record in an append-only log.
        </p>
      </div>
      <LoginPanel />
    </template>

    <template v-else>
      <div class="grid md:grid-cols-[1fr_20rem] gap-6 items-start">
        <!-- Session ledger -->
        <section class="panel leaf">
          <h2 class="panel-title">Your tables</h2>
          <div class="rule mx-4"></div>
          <ul v-if="sessions.length" class="divide-y divide-ink-800">
            <li v-for="s in sessions" :key="s.id">
              <router-link
                :to="{ name: 'session', params: { id: s.id } }"
                class="flex items-center justify-between px-4 py-3 hover:bg-ink-800/60 transition-colors group"
              >
                <div>
                  <div class="font-display text-lg text-bone-100 group-hover:text-ember-400 transition-colors">
                    {{ s.name }}
                  </div>
                  <div class="font-ledger text-xs text-bone-700">
                    {{ s.system_plugin_id }} v{{ s.system_plugin_version }}
                  </div>
                </div>
                <span
                  class="font-ledger text-[10px] uppercase tracking-widest px-2 py-0.5 rounded-sm"
                  :class="s.is_gm ? 'text-ember-400 border border-ember-600/40' : 'text-bone-500 border border-ink-600'"
                >
                  {{ s.is_gm ? 'GM' : 'Player' }}
                </span>
              </router-link>
            </li>
          </ul>
          <p v-else class="px-4 py-8 text-bone-700 text-sm italic">
            No tables yet — start one, or follow an invite link.
          </p>
        </section>

        <div class="space-y-6">
          <!-- New table -->
          <section class="panel leaf" style="animation-delay: 60ms">
            <h2 class="panel-title">Start a table</h2>
            <div class="rule mx-4"></div>
            <form class="p-4 space-y-3" @submit.prevent="createSession">
              <input v-model="name" class="field" placeholder="Table name" required maxlength="120" />
              <select v-model="pluginId" class="field" required>
                <option v-for="p in plugins.filter((p) => p.plugin_type === 'system')" :key="p.id" :value="p.id">
                  {{ p.id }} v{{ p.version }}
                </option>
              </select>
              <button class="btn-ember w-full" :disabled="busy || !name || !pluginId">Create</button>
            </form>
          </section>

          <!-- Join -->
          <section class="panel leaf" style="animation-delay: 120ms">
            <h2 class="panel-title">Join by invite</h2>
            <div class="rule mx-4"></div>
            <form class="p-4 space-y-3" @submit.prevent="join">
              <input v-model="joinId" class="field font-ledger text-sm" placeholder="Session link or id" />
              <button class="btn-ghost w-full" :disabled="!joinId.trim()">Join</button>
            </form>
          </section>

          <p v-if="error" class="text-blood-500 text-sm px-1">{{ error }}</p>
        </div>
      </div>
    </template>
  </div>
</template>
