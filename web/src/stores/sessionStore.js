// the live table: client-side projection folded from WS record frames,
// mirroring the kernel's apply (lib/fold.js), plus the command channel.
//
// protocol (kernel session/protocol.rs):
//   → { type: 'command', id, name, payload }
//   ← { type: 'snapshot', seq, world }
//   ← { type: 'record', record: { seq, at, cause, deltas } }
//   ← { type: 'error', command_id, message }   (issuing client only)
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { apiClient } from '@/lib/apiClient'
import { applyRecord } from '@/lib/fold'
import { useAuthStore } from '@/stores/authStore'

const WS_BASE = import.meta.env.VITE_WS_BASE_URL
const COMMAND_TIMEOUT_MS = 10_000
const FEED_LIMIT = 120

export const useSessionStore = defineStore('session', () => {
  const sessionId = ref(null)
  const detail = ref(null) // REST detail: name, members, is_gm, manifest
  const world = ref({}) // entity → component → value
  const seq = ref(0)
  const connected = ref(false)
  const lastError = ref(null)
  const feed = ref([]) // newest-first activity from the record stream

  let socket = null
  let closedByUser = false
  let backoffMs = 1000
  const pending = new Map() // command id → { resolve, reject, timer }

  const manifest = computed(() => detail.value?.manifest ?? null)
  const layout = computed(() => manifest.value?.sheet_layout ?? null)
  const isGm = computed(() => detail.value?.is_gm ?? false)

  /** entities that look like sheets: they carry the layout's name component. */
  const sheetEntities = computed(() => {
    const nameKey = layout.value?.nameComponent ?? 'core.name'
    return Object.entries(world.value)
      .filter(([, components]) => nameKey in components)
      .map(([id, components]) => ({
        id,
        name: components[nameKey],
        owner: components[layout.value?.ownerComponent]?.user_id ?? null,
      }))
  })

  async function enter(id) {
    leave()
    closedByUser = false
    sessionId.value = id
    // interim join-by-URL (prototype carryover, to be replaced by invite
    // tokens, docs/AUTH_SECURITY.md); join is an idempotent upsert.
    await apiClient.post(`/sessions/${id}/join`, undefined, 'join_session')
    detail.value = await apiClient.get(`/sessions/${id}`)
    await connect()
  }

  async function connect() {
    if (!sessionId.value || closedByUser) return
    const token = await useAuthStore().accessToken()
    if (!token) return

    const params = new URLSearchParams({ token })
    if (seq.value > 0) params.set('after_seq', String(seq.value))
    const ws = new WebSocket(`${WS_BASE}/sessions/${sessionId.value}/ws?${params}`)
    socket = ws

    ws.onopen = () => {
      connected.value = true
      backoffMs = 1000
    }

    ws.onmessage = (event) => {
      const frame = JSON.parse(event.data)
      if (frame.type === 'snapshot') {
        world.value = frame.world
        seq.value = frame.seq
      } else if (frame.type === 'record') {
        const record = frame.record
        if (record.seq <= seq.value) return // dedup across snapshot/broadcast handoff
        applyRecord(world.value, record)
        seq.value = record.seq
        harvest(record)
        settle(record.cause?.command_id, record)
      } else if (frame.type === 'error') {
        if (!settle(frame.command_id, null, frame.message)) {
          lastError.value = frame.message
        }
      }
    }

    ws.onclose = () => {
      connected.value = false
      if (socket === ws) socket = null
      if (!closedByUser) {
        setTimeout(connect, backoffMs)
        backoffMs = Math.min(backoffMs * 2, 10_000)
      }
    }
  }

  /** resolve/reject the pending command promise; true if one was waiting. */
  function settle(commandId, record, errorMessage) {
    if (!commandId || !pending.has(commandId)) return false
    const { resolve, reject, timer } = pending.get(commandId)
    clearTimeout(timer)
    pending.delete(commandId)
    if (errorMessage != null) reject(new Error(errorMessage))
    else resolve(record)
    return true
  }

  /** pull human-readable activity out of the record stream. */
  function harvest(record) {
    const rollKey = layout.value?.lastRollComponent
    let roll = null
    for (const delta of record.deltas) {
      if (delta.op === 'set' && delta.component === rollKey) roll = delta.value
    }
    feed.value.unshift({
      seq: record.seq,
      at: record.at,
      command: record.cause?.command ?? null,
      roll,
    })
    if (feed.value.length > FEED_LIMIT) feed.value.length = FEED_LIMIT
  }

  function sendCommand(name, payload = {}) {
    return new Promise((resolve, reject) => {
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        reject(new Error('not connected'))
        return
      }
      const id = crypto.randomUUID()
      const timer = setTimeout(() => {
        pending.delete(id)
        reject(new Error('command timed out'))
      }, COMMAND_TIMEOUT_MS)
      pending.set(id, { resolve, reject, timer })
      socket.send(JSON.stringify({ type: 'command', id, name, payload }))
    })
  }

  function leave() {
    closedByUser = true
    if (socket) socket.close()
    socket = null
    for (const [id] of pending) settle(id, null, 'left session')
    sessionId.value = null
    detail.value = null
    world.value = {}
    seq.value = 0
    feed.value = []
    lastError.value = null
    connected.value = false
  }

  return {
    sessionId,
    detail,
    world,
    seq,
    connected,
    lastError,
    feed,
    manifest,
    layout,
    isGm,
    sheetEntities,
    enter,
    leave,
    sendCommand,
  }
})
