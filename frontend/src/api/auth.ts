import axios from 'axios'

// 认证 API 始终指向云端服务器（不走本地 Tauri）
const AUTH_API_URL = 'https://zys-openclaw.com/api/v1'

const authClient = axios.create({
  baseURL: AUTH_API_URL,
  timeout: 30000,
  headers: { 'Content-Type': 'application/json' },
})

// 登录请求类型
export interface LoginRequest {
  enterpriseId: string
  email: string
  password: string
}

// 注册请求类型
export interface RegisterRequest {
  enterpriseId: string
  email: string
  name: string
  password: string
}

// 认证响应类型
export interface AuthResponse {
  token: string
  user: {
    id: string
    email: string
    name: string
    role: string
    enterpriseId: string
  }
  isNewUser?: boolean
}

// 发送验证码响应
export interface SendCodeResponse {
  message: string
  expiresIn: number
}

export const authAPI = {
  // 发送验证码
  sendCode: (email: string) =>
    authClient.post<SendCodeResponse>('/auth/send-code', { email }),

  // 验证码验证（统一注册/登录）
  verifyCode: (email: string, code: string) =>
    authClient.post<AuthResponse>('/auth/verify-code', { email, code }),

  // 设置密码（新用户注册后）
  setPassword: (email: string, password: string, name?: string) =>
    authClient.post('/auth/set-password', { email, password, name }),

  // 原有密码登录
  login: (enterpriseId: string, email: string, password: string) =>
    authClient.post<AuthResponse>('/auth/login', { enterpriseId, email, password }),

  // 原有注册
  register: (enterpriseId: string, email: string, name: string, password: string) =>
    authClient.post<AuthResponse>('/auth/register', { enterpriseId, email, name, password }),
}
