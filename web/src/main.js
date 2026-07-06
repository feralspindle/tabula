import { createApp } from 'vue'
import { createPinia } from 'pinia'

import '@fontsource/fraunces/400.css'
import '@fontsource/fraunces/600.css'
import '@fontsource/spectral/400.css'
import '@fontsource/spectral/500.css'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import './style.css'

import App from './App.vue'
import router from './router'
import { useAuthStore } from './stores/authStore'

const app = createApp(App)
app.use(createPinia())
app.use(router)

useAuthStore().init()

app.mount('#app')
