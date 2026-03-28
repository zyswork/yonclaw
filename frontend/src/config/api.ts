// API 地址配置
// Tauri 桌面端：通过 invoke() 直接调用，不走 HTTP
// Web 模式：自动检测当前 host（支持 Gateway 和 Nginx 反向代理）
const detectApiBase = () => {
  if (typeof window !== 'undefined' && window.location) {
    const { protocol, hostname, port } = window.location
    // 如果是本地开发或 Gateway 直连
    if (hostname === 'localhost' || hostname === '127.0.0.1') {
      return `${protocol}//${hostname}${port ? ':' + port : ''}`
    }
    // 生产环境：使用同源
    return `${protocol}//${hostname}${port ? ':' + port : ''}`
  }
  return 'http://127.0.0.1:9800'
}

export const API_BASE_URL = detectApiBase()
export const WS_URL = API_BASE_URL.replace('http', 'ws')
