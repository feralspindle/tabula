<script setup>
// the generic, schema-driven sheet renderer. renders ANY System plugin's
// sheet from its manifest-declared layout, no game-specific components here,
// that's the point (spec §5). widgets: text, number, stat (rollable), track,
// list, plus per-field action buttons.
import { computed, ref } from 'vue'
import { useSessionStore } from '@/stores/sessionStore'
import { useAuthStore } from '@/stores/authStore'

const props = defineProps({
  entityId: { type: String, required: true },
})

const session = useSessionStore()
const auth = useAuthStore()
const error = ref(null)
const advantage = ref('none') // none | advantage | disadvantage

const layout = computed(() => session.layout ?? { sections: [] })
const components = computed(() => session.world[props.entityId] ?? {})

const canEdit = computed(() => {
  if (session.isGm) return true
  const ownerKey = layout.value.ownerComponent
  if (!ownerKey) return true
  return components.value[ownerKey]?.user_id === auth.user?.id
})

function fieldValue(field) {
  const component = components.value[field.component]
  if (component === undefined) return undefined
  return field.field ? component?.[field.field] : component
}

function maxValue(field) {
  if (!field.maxField) return undefined
  return components.value[field.component]?.[field.maxField]
}

async function run(name, payload) {
  error.value = null
  try {
    await session.sendCommand(name, { entity: props.entityId, ...payload })
  } catch (e) {
    error.value = e.message
  }
}

function commitField(field, raw, kind) {
  let value = raw
  if (kind === 'number') {
    value = Number(raw)
    if (!Number.isFinite(value)) return
  }
  const current = fieldValue(field)
  if (value === current) return
  run(layout.value.editCommand ?? 'update-sheet-field', {
    component: field.component,
    field: field.field ?? '',
    value,
  })
}

function rollField(field) {
  if (!field.roll) return
  run(field.roll.command, { ...field.roll.args, advantage: advantage.value })
}

function statModifier(value) {
  if (typeof value !== 'number') return null
  const mod = Math.floor((value - 10) / 2)
  return mod >= 0 ? `+${mod}` : `${mod}`
}

// --- list widget editing (inventory) ---
function listItems(field) {
  const v = fieldValue(field)
  return Array.isArray(v) ? v : []
}

function commitList(field, items) {
  run(layout.value.editCommand ?? 'update-sheet-field', {
    component: field.component,
    field: field.field,
    value: items,
  })
}

const newItem = ref('')
function addItem(field) {
  const name = newItem.value.trim()
  if (!name) return
  commitList(field, [...listItems(field), { name, qty: 1 }])
  newItem.value = ''
}

function removeItem(field, index) {
  const items = [...listItems(field)]
  items.splice(index, 1)
  commitList(field, items)
}

const hasRollableSection = computed(() =>
  (layout.value.sections ?? []).some((s) => s.fields?.some((f) => f.roll)),
)
</script>

<template>
  <div class="space-y-4">
    <div
      v-for="section in layout.sections"
      :key="section.label"
      class="panel leaf"
    >
      <div class="flex items-center justify-between pr-4">
        <h3 class="panel-title">{{ section.label }}</h3>
        <!-- Advantage selector rides on the first rollable section -->
        <div
          v-if="hasRollableSection && section.fields?.some((f) => f.roll)"
          class="flex font-ledger text-[10px] uppercase tracking-wider border border-ink-600 rounded-sm overflow-hidden"
        >
          <button
            v-for="opt in ['disadvantage', 'none', 'advantage']"
            :key="opt"
            class="px-2 py-1 cursor-pointer transition-colors"
            :class="
              advantage === opt
                ? 'bg-ember-600 text-ink-950'
                : 'text-bone-500 hover:text-bone-100'
            "
            @click="advantage = opt"
          >
            {{ opt === 'none' ? '—' : opt === 'advantage' ? 'ADV' : 'DIS' }}
          </button>
        </div>
      </div>
      <div class="rule mx-4"></div>

      <div class="p-4 grid gap-3" :class="section.fields?.some((f) => f.widget === 'stat') ? 'grid-cols-3 sm:grid-cols-6' : 'sm:grid-cols-2'">
        <template v-for="field in section.fields" :key="field.component + (field.field ?? '')">
          <!-- STAT: value + modifier, rollable -->
          <div v-if="field.widget === 'stat'" class="text-center">
            <button
              class="w-full panel !bg-ink-800 py-2 group cursor-pointer hover:border-ember-600 transition-colors"
              :title="`Roll ${field.label} check`"
              @click="rollField(field)"
            >
              <div class="font-ledger text-[10px] uppercase tracking-widest text-bone-500 group-hover:text-ember-400">
                {{ field.label }}
              </div>
              <div class="font-display text-2xl text-bone-100">{{ fieldValue(field) ?? '—' }}</div>
              <div class="font-ledger text-xs text-verdigris-400">
                {{ statModifier(fieldValue(field)) ?? '' }}
              </div>
            </button>
            <input
              v-if="canEdit"
              class="field mt-1 text-center !py-0.5 text-sm"
              type="number"
              :value="fieldValue(field)"
              @change="commitField(field, $event.target.value, 'number')"
            />
          </div>

          <!-- TRACK: current / max -->
          <div v-else-if="field.widget === 'track'">
            <label class="font-ledger text-[10px] uppercase tracking-widest text-bone-500">
              {{ field.label }}
            </label>
            <div class="flex items-center gap-2 mt-1">
              <input
                class="field text-center"
                type="number"
                :value="fieldValue(field)"
                :disabled="!canEdit"
                @change="commitField(field, $event.target.value, 'number')"
              />
              <span class="text-bone-700 font-display">/</span>
              <input
                class="field text-center"
                type="number"
                :value="maxValue(field)"
                :disabled="!canEdit"
                @change="
                  commitField(
                    { component: field.component, field: field.maxField },
                    $event.target.value,
                    'number',
                  )
                "
              />
            </div>
          </div>

          <!-- LIST: array of {name, qty} -->
          <div v-else-if="field.widget === 'list'" class="sm:col-span-2">
            <label class="font-ledger text-[10px] uppercase tracking-widest text-bone-500">
              {{ field.label }}
            </label>
            <ul class="mt-1 divide-y divide-ink-800 border border-ink-700 rounded-sm">
              <li
                v-for="(item, i) in listItems(field)"
                :key="i"
                class="flex items-center justify-between px-3 py-1.5 text-sm"
              >
                <span>{{ item.name }}<span v-if="item.qty > 1" class="text-bone-700"> ×{{ item.qty }}</span></span>
                <button
                  v-if="canEdit"
                  class="text-bone-700 hover:text-blood-500 cursor-pointer font-ledger"
                  title="Remove"
                  @click="removeItem(field, i)"
                >
                  ✕
                </button>
              </li>
              <li v-if="!listItems(field).length" class="px-3 py-2 text-bone-700 text-sm italic">
                Empty
              </li>
            </ul>
            <form v-if="canEdit" class="flex gap-2 mt-2" @submit.prevent="addItem(field)">
              <input v-model="newItem" class="field text-sm" placeholder="Add item…" />
              <button class="btn-ghost text-xs" :disabled="!newItem.trim()">Add</button>
            </form>
          </div>

          <!-- TEXT / NUMBER -->
          <div v-else>
            <label class="font-ledger text-[10px] uppercase tracking-widest text-bone-500">
              {{ field.label }}
              <button
                v-if="field.action && canEdit"
                class="ml-1 text-ember-500 hover:text-ember-400 lowercase tracking-normal cursor-pointer"
                @click="run(field.action.command, {})"
              >
                [{{ field.action.label }}]
              </button>
            </label>
            <input
              class="field mt-1"
              :type="field.widget === 'number' ? 'number' : 'text'"
              :value="fieldValue(field)"
              :disabled="!canEdit"
              @change="commitField(field, $event.target.value, field.widget)"
            />
          </div>
        </template>
      </div>
    </div>

    <p v-if="error" class="text-blood-500 text-sm px-1">{{ error }}</p>
  </div>
</template>
