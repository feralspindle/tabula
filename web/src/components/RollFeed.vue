<script setup>
// the visible slice of the ledger: activity harvested from the record stream,
// with roll results rendered large. every entry is one applied LogRecord.
import { useSessionStore } from '@/stores/sessionStore'

const session = useSessionStore()

function time(at) {
  return new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}
</script>

<template>
  <div class="panel leaf">
    <h3 class="panel-title">Ledger</h3>
    <div class="rule mx-4"></div>
    <ul class="max-h-96 overflow-y-auto divide-y divide-ink-800">
      <li
        v-for="entry in session.feed"
        :key="entry.seq"
        class="px-4 py-2.5"
        :class="{ flare: entry.roll }"
      >
        <div class="flex items-baseline justify-between gap-2">
          <span class="font-ledger text-[10px] text-bone-700">#{{ entry.seq }}</span>
          <span class="font-ledger text-[10px] text-bone-700">{{ time(entry.at) }}</span>
        </div>
        <template v-if="entry.roll">
          <div class="flex items-baseline justify-between gap-3 mt-0.5">
            <span class="text-sm text-bone-300">{{ entry.roll.label }}</span>
            <span class="font-display text-2xl text-ember-400">
              {{ entry.roll.grand_total ?? entry.roll.total }}
            </span>
          </div>
          <div class="font-ledger text-xs text-bone-700">
            {{ entry.roll.expr }} = {{ entry.roll.total }}
            <template v-if="entry.roll.modifier != null">
              {{ entry.roll.modifier >= 0 ? '+' : '−' }}{{ Math.abs(entry.roll.modifier) }}
            </template>
          </div>
        </template>
        <div v-else class="text-sm text-bone-500 mt-0.5">
          {{ entry.command ?? 'change' }}
        </div>
      </li>
      <li v-if="!session.feed.length" class="px-4 py-6 text-bone-700 text-sm italic">
        Nothing recorded yet this sitting.
      </li>
    </ul>
  </div>
</template>
