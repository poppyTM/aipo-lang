import DefaultTheme from 'vitepress/theme'
import Layout from './Layout.vue'
import ProgressBoard from './components/ProgressBoard.vue'
import './custom.css'
import './progress.css'
export default {
  extends: DefaultTheme,
  Layout,
  enhanceApp({ app }) { app.component('ProgressBoard', ProgressBoard) }
}
