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

// 安全存储辅助：token/user 存到 Tauri 后端 SQLite，不用 localStorage（防 XSS）
const secureSet = async (key: string, value: string) => {
  try {
    const { invoke } = await import('@tauri-apps/api/tauri')
    await invoke('set_setting', { key: `auth.${key}`, value })
  } catch {
    // Tauri 未就绪时 fallback localStorage（开发模式）
    localStorage.setItem(key, value)
  }
}
const secureGet = async (key: string): Promise<string | null> => {
  try {
    const { invoke } = await import('@tauri-apps/api/tauri')
    return await invoke<string | null>('get_setting', { key: `auth.${key}` })
  } catch {
    return localStorage.getItem(key)
  }
}
const secureRemove = async (key: string) => {
  try {
    const { invoke } = await import('@tauri-apps/api/tauri')
    await invoke('set_setting', { key: `auth.${key}`, value: '' })
  } catch {
    localStorage.removeItem(key)
  }
}

export const useAuthStore = create<AuthStore>((set) => ({
  user: null,
  // 初始状态从 localStorage 快速读取（hydrate 会从安全存储覆盖）
  token: typeof window !== 'undefined' ? localStorage.getItem('token') : null,
  isLoggedIn: typeof window !== 'undefined' ? !!localStorage.getItem('token') : false,
  nickname: '',
  avatarUrl: '',
  bio: '',
  setUser: (user) => set({ user }),
  setToken: (token) => {
    localStorage.setItem('token', token) // 快速 UI 响应
    secureSet('token', token) // 安全持久化
    markHadLogin()
    set({ token, isLoggedIn: true })
  },
  login: (token, user) => {
    localStorage.setItem('token', token)
    localStorage.setItem('user', JSON.stringify(user))
    secureSet('token', token)
    secureSet('user', JSON.stringify(user))
    markHadLogin()
    set({ token, user, isLoggedIn: true })
    // 同步用户信息到 Tauri 后端（遥测 + 本地 profile 初始化）
    if (user?.email || user?.name) {
      import('@tauri-apps/api/tauri').then(({ invoke }) => {
        invoke('set_setting', { key: 'user_id', value: user.email || user.id || '' }).catch(() => {})
        invoke('set_setting', { key: 'user_name', value: user.name || '' }).catch(() => {})
        invoke('set_setting', { key: 'user_email', value: user.email || '' }).catch(() => {})
        // 新设备首次登录：如果本地没有 profile，用服务端的 name 初始化
        invoke<{ nickname: string; bio: string }>('get_user_profile').then((profile) => {
          if (!profile?.nickname && user.name) {
            invoke('save_user_profile', { nickname: user.name, bio: '' }).catch(() => {})
            set({ nickname: user.name })
          }
        }).catch(() => {})
      }).catch(() => {})
    }
  },
  logout: () => {
    localStorage.removeItem('token')
    localStorage.removeItem('user')
    localStorage.setItem('had_login', 'true')
    secureRemove('token')
    secureRemove('user')
    set({ user: null, token: null, isLoggedIn: false })
    useEnterpriseStore.setState({ enterprise: null })
    // ProtectedPage 检测到 !isLoggedIn + had_login → 重定向到 /login
    // LoginPage 检测到 had_login → 显示登录页（不会自动跳走）
  },
  hydrate: () => {
    // 优先从 localStorage 快速恢复（同步，UI 无闪烁）
    const token = localStorage.getItem('token')
    const userStr = localStorage.getItem('user')
    if (token && userStr) {
      try {
        const user = JSON.parse(userStr) as User
        markHadLogin()
        set({ token, user, isLoggedIn: true })
        if (user?.email || user?.name) {
          import('@tauri-apps/api/tauri').then(({ invoke }) => {
            invoke('set_setting', { key: 'user_id', value: user.email || user.id || '' }).catch(() => {})
            invoke('set_setting', { key: 'user_name', value: user.name || '' }).catch(() => {})
            invoke('set_setting', { key: 'user_email', value: user.email || '' }).catch(() => {})
          }).catch(() => {})
        }
      } catch {
        localStorage.removeItem('token')
        localStorage.removeItem('user')
        set({ token: null, user: null, isLoggedIn: false })
      }
    } else if (token) {
      markHadLogin()
      set({ token, isLoggedIn: true })
    } else {
      // localStorage 没有 → 尝试从安全存储恢复（异步）
      secureGet('token').then(secToken => {
        if (!secToken) return
        secureGet('user').then(secUser => {
          if (secToken && secUser) {
            try {
              const user = JSON.parse(secUser) as User
              // 回写 localStorage 以备下次快速恢复
              localStorage.setItem('token', secToken)
              localStorage.setItem('user', secUser)
              markHadLogin()
              set({ token: secToken, user, isLoggedIn: true })
            } catch {}
          }
        })
      })
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
