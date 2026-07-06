<script setup>
// dice roller ported in spirit from hexmapper: expression input, quick dice,
// result surfaced through the record feed (RollFeed). generic: the command
// comes from the plugin's layout (`dice.command`).
import { ref, computed } from 'vue'
import { useSessionStore } from '@/stores/sessionStore'

const props = defineProps({
  entityId: { type: String, default: null },
})

const session = useSessionStore()
const expr = ref('')
const error = ref(null)
const rolling = ref(false)

const dice = computed(() => session.layout?.dice ?? null)
const QUICK = ['1d4', '1d6', '1d8', '1d10', '1d12', '1d20', '1d100', '2d6']

async function roll(expression) {
  const e = (expression ?? expr.value).trim()
  if (!e || !dice.value || !props.entityId) return
  error.value = null
  rolling.value = true
  try {
    await session.sendCommand(dice.value.command, {
      entity: props.entityId,
      [dice.value.exprArg]: e,
    })
    expr.value = ''
  } catch (err) {
    error.value = err.message
  } finally {
    rolling.value = false
  }
}
</script>

<template>
  <div v-if="dice" class="panel leaf">
    <h3 class="panel-title">Dice</h3>
    <div class="rule mx-4"></div>
    <div class="p-4 space-y-3">
      <p v-if="!entityId" class="text-bone-700 text-sm italic">Select a sheet to roll.</p>
      <template v-else>
        <form class="flex gap-2" @submit.prevent="roll()">
          <input
            v-model="expr"
            class="field font-ledger text-sm"
            placeholder="2d6+1, 4d6kh3, 2d20kl1…"
            maxlength="80"
          />
          <button class="btn-ember" :disabled="rolling || !expr.trim()">Roll</button>
        </form>
        <div class="grid grid-cols-4 gap-1.5">
          <button
            v-for="q in QUICK"
            :key="q"
            class="btn-ghost !px-1 font-ledger text-xs"
            :disabled="rolling"
            @click="roll(q)"
          >
            {{ q }}
          </button>
        </div>
        <p v-if="error" class="text-blood-500 text-sm">{{ error }}</p>
      </template>
    </div>
  </div>
</template>
