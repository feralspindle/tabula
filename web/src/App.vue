<script setup>
import { useAuthStore } from '@/stores/authStore'
import { useRouter } from 'vue-router'

const auth = useAuthStore()
const router = useRouter()

async function signOut() {
  await auth.signOut()
  router.push({ name: 'home' })
}
</script>

<template>
  <div class="min-h-screen flex flex-col">
    <header class="border-b border-ink-700 bg-ink-900/80 backdrop-blur-sm sticky top-0 z-40">
      <div class="max-w-6xl mx-auto px-4 h-14 flex items-center justify-between">
        <router-link :to="{ name: 'home' }" class="flex items-baseline gap-3 group">
          <span class="font-display text-2xl tracking-[0.28em] text-bone-100 group-hover:text-ember-400 transition-colors">
            TABVLA
          </span>
          <span class="font-ledger text-[10px] uppercase tracking-widest text-bone-700 hidden sm:inline">
            the table is a ledger
          </span>
        </router-link>
        <div v-if="auth.isAuthenticated" class="flex items-center gap-3">
          <span class="text-sm text-bone-500 font-body">{{ auth.displayName }}</span>
          <button class="btn-ghost text-xs" @click="signOut">Sign out</button>
        </div>
      </div>
    </header>

    <main class="flex-1">
      <router-view />
    </main>

    <footer class="border-t border-ink-800 py-3">
      <p class="text-center font-ledger text-[10px] tracking-widest text-bone-700 uppercase">
        tabula · system-agnostic vtt engine
      </p>
    </footer>
  </div>
</template>
