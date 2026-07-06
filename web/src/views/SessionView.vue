<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useSessionStore } from '@/stores/sessionStore'
import { useAuthStore } from '@/stores/authStore'
import LoginPanel from '@/components/LoginPanel.vue'
import SheetRenderer from '@/components/SheetRenderer.vue'
import DiceRoller from '@/components/DiceRoller.vue'
import RollFeed from '@/components/RollFeed.vue'

const props = defineProps({ id: { type: String, required: true } })

const session = useSessionStore()
const auth = useAuthStore()
const error = ref(null)
const activeSheet = ref(null)
const showCreate = ref(false)
const createForm = ref({})
const creating = ref(false)

async function enter() {
  error.value = null
  try {
    await session.enter(props.id)
  } catch (e) {
    error.value = e.message
  }
}

onMounted(async () => {
  await auth.init()
  if (auth.isAuthenticated) await enter()
})
watch(
  () => auth.isAuthenticated,
  async (ok) => {
    if (ok && !session.sessionId) await enter()
  },
)
onUnmounted(() => session.leave())

// keep a sensible active sheet selected.
watch(
  () => session.sheetEntities,
  (sheets) => {
    if (!sheets.find((s) => s.id === activeSheet.value)) {
      activeSheet.value = sheets[0]?.id ?? null
    }
  },
  { deep: true },
)

const createSpec = computed(() => session.layout?.create ?? null)

async function createSheet() {
  if (!createSpec.value) return
  creating.value = true
  error.value = null
  try {
    const record = await session.sendCommand(createSpec.value.command, { ...createForm.value })
    // the spawned entity becomes the active sheet.
    const spawned = record?.deltas?.find((d) => d.op === 'spawn')
    if (spawned) activeSheet.value = spawned.entity
    showCreate.value = false
    createForm.value = {}
  } catch (e) {
    error.value = e.message
  } finally {
    creating.value = false
  }
}

async function copyInvite() {
  await navigator.clipboard.writeText(window.location.href)
}
</script>

<template>
  <div class="max-w-6xl mx-auto px-4 py-6">
    <template v-if="!auth.loading && !auth.isAuthenticated">
      <p class="text-center text-bone-500 mb-6">Sign in to take your seat at this table.</p>
      <LoginPanel />
    </template>

    <template v-else-if="session.detail">
      <!-- Table header -->
      <div class="flex flex-wrap items-baseline justify-between gap-3 mb-5 leaf">
        <div class="flex items-baseline gap-4">
          <h1 class="font-display text-3xl text-bone-100">{{ session.detail.name }}</h1>
          <span class="font-ledger text-xs text-bone-700">
            {{ session.detail.system_plugin_id }} v{{ session.detail.system_plugin_version }}
          </span>
          <span
            class="font-ledger text-[10px] uppercase tracking-widest"
            :class="session.connected ? 'text-verdigris-400' : 'text-blood-500'"
          >
            {{ session.connected ? `● live · seq ${session.seq}` : '○ reconnecting…' }}
          </span>
        </div>
        <button class="btn-ghost text-xs" @click="copyInvite">Copy invite link</button>
      </div>

      <div class="grid lg:grid-cols-[15rem_1fr_20rem] gap-5 items-start">
        <!-- Sheets + members -->
        <div class="space-y-5">
          <section class="panel leaf">
            <div class="flex items-center justify-between pr-3">
              <h3 class="panel-title">{{ session.layout?.title ?? 'Sheets' }}</h3>
              <button
                v-if="createSpec"
                class="text-ember-500 hover:text-ember-400 font-display text-lg cursor-pointer"
                :title="createSpec.label"
                @click="showCreate = !showCreate"
              >
                +
              </button>
            </div>
            <div class="rule mx-4"></div>

            <form v-if="showCreate" class="p-3 space-y-2 border-b border-ink-800" @submit.prevent="createSheet">
              <input
                v-for="f in createSpec.fields"
                :key="f.name"
                v-model="createForm[f.name]"
                class="field text-sm"
                :placeholder="f.label"
                :required="f.required ?? false"
              />
              <button class="btn-ember w-full text-xs" :disabled="creating">
                {{ createSpec.label }}
              </button>
            </form>

            <ul class="divide-y divide-ink-800">
              <li v-for="sheet in session.sheetEntities" :key="sheet.id">
                <button
                  class="w-full text-left px-4 py-2 text-sm transition-colors cursor-pointer"
                  :class="
                    sheet.id === activeSheet
                      ? 'text-ember-400 bg-ink-800/70 border-l-2 border-ember-600'
                      : 'text-bone-300 hover:bg-ink-800/40'
                  "
                  @click="activeSheet = sheet.id"
                >
                  {{ sheet.name }}
                </button>
              </li>
              <li v-if="!session.sheetEntities.length" class="px-4 py-4 text-bone-700 text-sm italic">
                No sheets yet.
              </li>
            </ul>
          </section>

          <section class="panel leaf" style="animation-delay: 60ms">
            <h3 class="panel-title">At the table</h3>
            <div class="rule mx-4"></div>
            <ul class="p-3 space-y-1 text-sm">
              <li class="text-bone-300">
                <span class="text-ember-500 font-ledger text-[10px] uppercase mr-1">GM</span>
                {{
                  session.detail.members.find((m) => m.user_id === session.detail.owner_id)
                    ?.display_name ?? 'Game Master'
                }}
              </li>
              <li
                v-for="m in session.detail.members.filter((m) => m.user_id !== session.detail.owner_id)"
                :key="m.user_id"
                class="text-bone-500"
              >
                {{ m.display_name || 'Adventurer' }}
              </li>
            </ul>
          </section>
        </div>

        <!-- Active sheet -->
        <div>
          <SheetRenderer v-if="activeSheet" :entity-id="activeSheet" :key="activeSheet" />
          <div v-else class="panel leaf p-10 text-center text-bone-700 italic">
            {{ createSpec ? `No sheet selected — create one with “+”.` : 'Nothing here yet.' }}
          </div>
        </div>

        <!-- Dice + ledger -->
        <div class="space-y-5">
          <DiceRoller :entity-id="activeSheet" />
          <RollFeed style="animation-delay: 60ms" />
        </div>
      </div>

      <p v-if="error || session.lastError" class="text-blood-500 text-sm mt-4">
        {{ error ?? session.lastError }}
      </p>
    </template>

    <div v-else class="text-center py-20">
      <p v-if="error" class="text-blood-500">{{ error }}</p>
      <p v-else class="font-ledger text-sm text-bone-700">opening the ledger…</p>
    </div>
  </div>
</template>
