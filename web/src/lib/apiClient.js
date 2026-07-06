// ported from hexmapper: fetch wrapper carrying auth + forensic headers.
import { supabase } from '@/lib/supabase'

const BASE_URL = import.meta.env.VITE_API_BASE_URL

// one client id per tab/load; rides along in request metadata for forensics.
const CLIENT_ID = crypto.randomUUID()
const APP_VERSION = import.meta.env.VITE_APP_VERSION || 'dev'

export class ApiError extends Error {
  constructor(status, message) {
    super(message)
    this.status = status
  }
}

async function request(method, path, body, intent) {
  const {
    data: { session },
  } = await supabase.auth.getSession()
  const requestId = crypto.randomUUID()

  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: {
      'Content-Type': 'application/json',
      ...(session?.access_token ? { Authorization: `Bearer ${session.access_token}` } : {}),
      'X-Request-Id': requestId,
      'X-Client-Id': CLIENT_ID,
      'X-App-Version': APP_VERSION,
      ...(intent ? { 'X-Intent': intent } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })

  if (!res.ok) {
    const err = await res.json().catch(() => ({}))
    throw new ApiError(res.status, err.message ?? res.statusText)
  }

  return res.status === 204 ? null : res.json()
}

export const apiClient = {
  get: (path) => request('GET', path),
  post: (path, body, intent) => request('POST', path, body, intent),
  put: (path, body, intent) => request('PUT', path, body, intent),
  delete: (path, intent) => request('DELETE', path, undefined, intent),
}
