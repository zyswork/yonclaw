import { create } from 'zustand'
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
  setUser: (user: User) => void
  setToken: (token: string) => void
  login: (token: string, user: User) => void
  logout: () => void
  /** 从 localStorage 恢复登录状态 */
  hydrate: () => void
}

export const useAuthStore = create<AuthStore>((set) => ({
  user: null,
  token: typeof window !== 'undefined' ? localStorage.getItem('token') : null,
  isLoggedIn: typeof window !== 'undefined' ? !!localStorage.getItem('token') : false,
  setUser: (user) => set({ user }),
  setToken: (token) => {
    localStorage.setItem('token', token)
    set({ token, isLoggedIn: true })
  },
  login: (token, user) => {
    localStorage.setItem('token', token)
    localStorage.setItem('user', JSON.stringify(user))
    set({ token, user, isLoggedIn: true })
  },
  logout: () => {
    localStorage.removeItem('token')
    localStorage.removeItem('user')
    set({ user: null, token: null, isLoggedIn: false })
    useEnterpriseStore.setState({ enterprise: null })
  },
  hydrate: () => {
    const token = localStorage.getItem('token')
    const userStr = localStorage.getItem('user')
    if (token && userStr) {
      try {
        const user = JSON.parse(userStr) as User
        set({ token, user, isLoggedIn: true })
      } catch {
        // JSON 解析失败，清理无效数据
        localStorage.removeItem('token')
        localStorage.removeItem('user')
        set({ token: null, user: null, isLoggedIn: false })
      }
    } else if (!token) {
      set({ token: null, user: null, isLoggedIn: false })
    }
  },
}))
