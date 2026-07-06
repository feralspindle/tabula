import { createClient } from '@supabase/supabase-js'

const url = import.meta.env.VITE_SUPABASE_URL
const key = import.meta.env.VITE_SUPABASE_PUBLISHABLE_DEFAULT_KEY

if (!url || !key) {
  console.warn(
    '[tabula] Supabase env vars missing. Copy .env.example to .env and fill in your project credentials.',
  )
}

export const supabase = createClient(url ?? '', key ?? '')
