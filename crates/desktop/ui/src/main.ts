import './assets/main.css'

import { createApp } from 'vue'
import App from './App.vue'
import { armBootFallback } from './composables/boot'

// 全局右键抑制：空白区域不出现浏览器原生菜单（复制/查看源代码等与应用风格冲突）
// 输入类元素放行——保留浏览器原生的复制/粘贴/拼写检查（输入框核心交互不可丢）；
// 已自定义菜单的区域（文件树/主机栏）各自 preventDefault，不受此监听影响
window.addEventListener('contextmenu', (e) => {
  const target = e.target as HTMLElement
  if (target.closest('input, textarea')) return
  e.preventDefault()
})

// 窗口不可见期间设兜底：setup/onMounted 失败或挂起时窗口不永久隐藏
// （初始化完成由 App.vue 调 completeBoot 取消兜底并显示窗口）
armBootFallback()

createApp(App).mount('#app')
