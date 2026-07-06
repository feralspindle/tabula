// ported from hexmapper's authStore, trimmed to the MVP surface.
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { supabase } from '@/lib/supabase'

export const useAuthStore = defineStore('auth', () => {
  const user = ref(null)
  const loading = ref(true)

  const isAuthenticated = computed(() => !!user.value)

  const displayName = computed(() => {
    if (!user.value) return null
    const m = user.value.user_metadata ?? {}
    return (
      m.full_name ||
      m.global_name ||
      m.custom_claims?.global_name ||
      m.name ||
      m.user_name ||
      user.value.email ||
      'Adventurer'
    )
  })

  let _initPromise = null
  function init() {
    if (_initPromise) return _initPromise
    _initPromise = _doInit()
    return _initPromise
  }

  async function _doInit() {
    loading.value = true
    const { data } = await supabase.auth.getSession()
    user.value = data.session?.user ?? null
    loading.value = false

    supabase.auth.onAuthStateChange((_event, session) => {
      user.value = session?.user ?? null
    })
  }

  async function signInWithEmail(email, password) {
    const { error } = await supabase.auth.signInWithPassword({ email, password })
    if (error) throw error
  }

  async function signUpWithEmail(username, email, password) {
    const { data, error } = await supabase.auth.signUp({
      email,
      password,
      options: { data: { full_name: username } },
    })
    if (error) throw error
    return { needsConfirmation: !data.session }
  }

  async function signInWithDiscord() {
    const { error } = await supabase.auth.signInWithOAuth({
      provider: 'discord',
      options: {
        redirectTo: `${window.location.origin}/auth/callback`,
        scopes: 'identify email',
      },
    })
    if (error) throw error
  }

  async function signOut() {
    const { error } = await supabase.auth.signOut()
    if (error) throw error
    user.value = null
  }

  async function accessToken() {
    const { data } = await supabase.auth.getSession()
    return data.session?.access_token ?? null
  }

  return {
    user,
    loading,
    isAuthenticated,
    displayName,
    init,
    signInWithEmail,
    signUpWithEmail,
    signInWithDiscord,
    signOut,
    accessToken,
  }
})
