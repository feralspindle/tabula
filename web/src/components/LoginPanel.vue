<script setup>
import { ref } from 'vue'
import { useAuthStore } from '@/stores/authStore'

const auth = useAuthStore()
const mode = ref('signin') // signin | signup
const username = ref('')
const email = ref('')
const password = ref('')
const error = ref(null)
const notice = ref(null)
const busy = ref(false)

async function submit() {
  error.value = null
  notice.value = null
  busy.value = true
  try {
    if (mode.value === 'signin') {
      await auth.signInWithEmail(email.value, password.value)
    } else {
      const { needsConfirmation } = await auth.signUpWithEmail(
        username.value,
        email.value,
        password.value,
      )
      if (needsConfirmation) notice.value = 'Check your email to confirm your account.'
    }
  } catch (e) {
    error.value = e.message
  } finally {
    busy.value = false
  }
}

async function discord() {
  error.value = null
  try {
    await auth.signInWithDiscord()
  } catch (e) {
    error.value = e.message
  }
}
</script>

<template>
  <div class="panel max-w-sm mx-auto leaf">
    <h2 class="panel-title">{{ mode === 'signin' ? 'Enter the record' : 'Open a new account' }}</h2>
    <div class="rule mx-4"></div>
    <form class="p-4 space-y-3" @submit.prevent="submit">
      <input
        v-if="mode === 'signup'"
        v-model="username"
        class="field"
        placeholder="Display name"
        autocomplete="nickname"
      />
      <input v-model="email" class="field" type="email" placeholder="Email" autocomplete="email" required />
      <input
        v-model="password"
        class="field"
        type="password"
        placeholder="Password"
        :autocomplete="mode === 'signin' ? 'current-password' : 'new-password'"
        required
      />
      <button class="btn-ember w-full" type="submit" :disabled="busy">
        {{ mode === 'signin' ? 'Sign in' : 'Sign up' }}
      </button>
      <button class="btn-ghost w-full" type="button" @click="discord">Continue with Discord</button>
      <p v-if="error" class="text-blood-500 text-sm">{{ error }}</p>
      <p v-if="notice" class="text-verdigris-400 text-sm">{{ notice }}</p>
      <p class="text-center text-sm text-bone-500">
        <button
          type="button"
          class="underline decoration-ink-500 hover:text-ember-400 cursor-pointer"
          @click="mode = mode === 'signin' ? 'signup' : 'signin'"
        >
          {{ mode === 'signin' ? 'Need an account? Sign up' : 'Have an account? Sign in' }}
        </button>
      </p>
    </form>
  </div>
</template>
