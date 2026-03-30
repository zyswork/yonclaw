import { create } from 'zustand'

export type Theme = 'light' | 'dark' | 'system'

function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function resolveInitial(): Theme {
  const saved = localStorage.getItem('xianzhu.theme') as Theme | null
  if (saved && ['light', 'dark', 'system'].includes(saved)) return saved
  return 'dark' // 默认深色主题
}

function applyTheme(theme: Theme) {
  const resolved = theme === 'system' ? getSystemTheme() : theme
  document.documentElement.setAttribute('data-theme', resolved)
}

interface ThemeState {
  theme: Theme
  setTheme: (t: Theme) => void
  resolvedTheme: () => 'light' | 'dark'
}

// 模块级标志，确保系统主题监听器只注册一次
let themeListenerRegistered = false

export const useTheme = create<ThemeState>((set, get) => {
  // 初始化时应用主题
  const initial = resolveInitial()
  applyTheme(initial) // 立即应用，避免闪屏

  // 监听系统主题变化（仅注册一次，避免内存泄漏）
  if (!themeListenerRegistered) {
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      if (get().theme === 'system') applyTheme('system')
    })
    themeListenerRegistered = true
  }

  return {
    theme: initial,
    setTheme: (t: Theme) => {
      localStorage.setItem('xianzhu.theme', t)
      applyTheme(t)
      set({ theme: t })
    },
    resolvedTheme: () => {
      const { theme } = get()
      return theme === 'system' ? getSystemTheme() : theme
    },
  }
})
