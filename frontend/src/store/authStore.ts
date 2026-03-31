import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/tauri'
import { useEnterpriseStore } from './enterpriseStore'

export interface User {
  id: string
  email: string
  name: string
  role: string
  enterpriseId: string
}

interface AuthStore {
  user: User | null
  token: string | null
  isLoggedIn: boolean
  nickname: string
  avatarUrl: string
  bio: string
  setUser: (user: User) => void
  setToken: (token: string) => void
  login: (token: string, user: User) => void
  logout: () => void
  /** 从 localStorage 恢复登录状态 */
  hydrate: () => void
  /** 合并个人资料字段到 state */
  setProfile: (p: { nickname?: string; avatarUrl?: string; bio?: string }) => void
  /** 从后端加载个人资料 */
  loadProfile: () => Promise<void>
}

/** 标记"曾经登录过"，logout 后不清除，用于区分"从未登录"和"登出" */
function markHadLogin() {
  localStorage.setItem('had_login', 'true')
}

export const useAuthStore = create<AuthStore>((set) => ({
  user: null,
  token: typeof window !== 'undefined' ? localStorage.getItem('token') : null,
  isLoggedIn: typeof window !== 'undefined' ? !!localStorage.getItem('token') : false,
  nickname: '',
  avatarUrl: '',
  bio: '',
  setUser: (user) => set({ user }),
  setToken: (token) => {
    localStorage.setItem('token', token)
    markHadLogin()
    set({ token, isLoggedIn: true })
  },
  login: (token, user) => {
    localStorage.setItem('token', token)
    localStorage.setItem('user', JSON.stringify(user))
    markHadLogin()
    set({ token, user, isLoggedIn: true })
    // 同步用户信息到 Tauri 后端（遥测用）
    if (user?.email || user?.name) {
      import('@tauri-apps/api/tauri').then(({ invoke }) => {
        invoke('set_setting', { key: 'user_id', value: user.email || user.id || '' }).catch(() => {})
        invoke('set_setting', { key: 'user_name', value: user.name || '' }).catch(() => {})
        invoke('set_setting', { key: 'user_email', value: user.email || '' }).catch(() => {})
      }).catch(() => {})
    }
  },
  logout: () => {
    localStorage.removeItem('token')
    localStorage.removeItem('user')
    localStorage.setItem('had_login', 'true')
    set({ user: null, token: null, isLoggedIn: false })
    useEnterpriseStore.setState({ enterprise: null })
    // ProtectedPage 检测到 !isLoggedIn + had_login → 重定向到 /login
    // LoginPage 检测到 had_login → 显示登录页（不会自动跳走）
  },
  hydrate: () => {
    const token = localStorage.getItem('token')
    const userStr = localStorage.getItem('user')
    if (token && userStr) {
      try {
        const user = JSON.parse(userStr) as User
        markHadLogin()
        set({ token, user, isLoggedIn: true })
        // 确保 Tauri 后端也有用户信息（遥测用）
        if (user?.email || user?.name) {
          import('@tauri-apps/api/tauri').then(({ invoke }) => {
            invoke('set_setting', { key: 'user_id', value: user.email || user.id || '' }).catch(() => {})
            invoke('set_setting', { key: 'user_name', value: user.name || '' }).catch(() => {})
            invoke('set_setting', { key: 'user_email', value: user.email || '' }).catch(() => {})
          }).catch(() => {})
        }
      } catch {
        // JSON 解析失败，清理无效数据
        localStorage.removeItem('token')
        localStorage.removeItem('user')
        set({ token: null, user: null, isLoggedIn: false })
      }
    } else if (token) {
      // 有 token 但没有 user JSON（兼容 setToken 路径）
      markHadLogin()
      set({ token, isLoggedIn: true })
    } else {
      set({ token: null, user: null, isLoggedIn: false })
    }
  },
  setProfile: (p) => set((state) => ({
    nickname: p.nickname ?? state.nickname,
    avatarUrl: p.avatarUrl ?? state.avatarUrl,
    bio: p.bio ?? state.bio,
  })),
  loadProfile: async () => {
    try {
      const profile = await invoke<{ nickname: string; bio: string }>('get_user_profile')
      const avatarBase64 = await invoke<string | null>('get_user_avatar')
      set({
        nickname: profile?.nickname || '',
        bio: profile?.bio || '',
        avatarUrl: avatarBase64 || '',
      })
    } catch {
      // 后端尚未实现或无数据，忽略
    }
  },
}))
