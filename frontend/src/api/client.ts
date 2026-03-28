import axios from 'axios'
import { API_BASE_URL } from '../config/api'

export const apiClient = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
})

// 添加请求拦截器，自动添加 token
apiClient.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) {
    config.headers.Authorization = `Bearer ${token}`
  }
  return config
})

// 添加响应拦截器，处理 401 错误（自动登出）
apiClient.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      // 仅对非认证端点触发自动登出（避免登录请求的 401 也清空状态）
      const url = error.config?.url || ''
      if (!url.includes('/auth/')) {
        localStorage.removeItem('token')
        localStorage.removeItem('user')
        window.location.reload()
      }
    }
    return Promise.reject(error)
  }
)
