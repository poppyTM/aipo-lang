<script setup lang="ts">
import DefaultTheme from 'vitepress/theme'
import { ref, onMounted } from 'vue'

const focus = ref(false)
const comfortable = ref(false)

function sync() {
  if (typeof document === 'undefined') return
  document.documentElement.classList.toggle('aipo-focus', focus.value)
  document.documentElement.classList.toggle('aipo-comfortable', comfortable.value)
}
function save() {
  sync()
  try {
    localStorage.setItem('aipo-reading-focus', focus.value ? '1' : '0')
    localStorage.setItem('aipo-reading-spacious', comfortable.value ? '1' : '0')
  } catch { /* Settings work without persistent storage. */ }
}
onMounted(() => {
  try {
    focus.value = localStorage.getItem('aipo-reading-focus') === '1'
    comfortable.value = localStorage.getItem('aipo-reading-spacious') === '1'
  } catch { /* Private browsing may disallow storage. */ }
  sync()
})
function toggleFocus() { focus.value = !focus.value; save() }
function toggleComfort() { comfortable.value = !comfortable.value; save() }
function reset() { focus.value = false; comfortable.value = false; save() }
</script>

<template>
  <DefaultTheme.Layout>
    <template #nav-bar-content-after>
      <details class="aipo-reading-panel">
        <summary aria-label="Abrir opções de leitura">Leitura</summary>
        <div class="aipo-reading-menu" role="group" aria-label="Ajustes de leitura">
          <button type="button" :aria-pressed="focus" @click="toggleFocus">
            {{ focus ? 'Desativar foco' : 'Foco no conteúdo' }}
          </button>
          <button type="button" :aria-pressed="comfortable" @click="toggleComfort">
            {{ comfortable ? 'Tamanho normal' : 'Texto maior' }}
          </button>
          <button type="button" @click="reset">Restaurar</button>
        </div>
      </details>
    </template>
  </DefaultTheme.Layout>
</template>
