import { HashRouter, Routes, Route, Navigate } from 'react-router-dom'
import React, { lazy, Suspense, useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useBackendConnection } from './hooks/useBackendConnection'
import { useI18n } from './i18n'
import { useAuthStore } from './store/authStore'
import SplashScreen from './components/SplashScreen'
import { ToastContainer } from './hooks/useToast'
import { ConfirmDialog } from './hooks/useConfirm'
import { ApprovalDialog } from './hooks/useApproval'
import SetupPage from './pages/SetupPage'
import { useUpdater } from './hooks/useUpdater'
import Layout from './components/Layout'
import OfflineBanner from './components/OfflineBanner'
import CommandPalette from './components/CommandPalette'

// 懒加载页面组件
const Dashboard = lazy(() => import('./pages/Dashboard'))
const AgentListPage = lazy(() => import('./pages/AgentListPage'))
const AgentDetailPage = lazy(() => import('./pages/AgentDetailPage'))
const AgentCreatePage = lazy(() => import('./pages/AgentCreatePage'))
const SettingsPage = lazy(() => import('./pages/SettingsPage'))
const CronPage = lazy(() => import('./pages/CronPage'))
const SkillsPage = lazy(() => import('./pages/SkillsPage'))
const MemoryPage = lazy(() => import('./pages/MemoryPage'))
const AuditLogPage = lazy(() => import('./pages/AuditLogPage'))
const TokenMonitoringPage = lazy(() => import('./pages/TokenMonitoringPage'))
const DoctorPage = lazy(() => import('./pages/DoctorPage'))
const ChannelsPage = lazy(() => import('./pages/ChannelsPage'))
const PluginsPage = lazy(() => import('./pages/PluginsPage'))
const PlazaPage = lazy(() => import('./pages/PlazaPage'))
const GroupChatPage = lazy(() => import('./pages/GroupChatPage'))
const LoginPage = lazy(() => import('./pages/LoginPage'))
const ModelComparePage = lazy(() => import('./pages/ModelComparePage'))

function PageLoader() {
  const { t } = useI18n()
  return (
    <div style={{ padding: '24px', textAlign: 'center', color: 'var(--text-muted)' }}>
      {t('common.loading')}
    </div>
  )
}

class ErrorBoundary extends React.Component<{ children: React.ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  static getDerivedStateFromError(error: Error) { return { error } }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // 本地持久化一份错误快照，供后续"提交 bug"时收集
    try {
      const snapshot = {
        ts: new Date().toISOString(),
        ua: navigator.userAgent,
        url: window.location.href,
        message: error.message,
        stack: error.stack,
        componentStack: info.componentStack,
      }
      const key = 'xianzhu.lastCrash'
      localStorage.setItem(key, JSON.stringify(snapshot))
    } catch {}
  }

  copyReport = () => {
    if (!this.state.error) return
    const report = [
      `XianZhu crash report`,
      `Time: ${new Date().toISOString()}`,
      `URL: ${window.location.href}`,
      `User Agent: ${navigator.userAgent}`,
      ``,
      `Message: ${this.state.error.message}`,
      ``,
      `Stack:`,
      this.state.error.stack || '(no stack)',
    ].join('\n')
    navigator.clipboard.writeText(report).then(() => {
      alert('错误报告已复制到剪贴板 / Crash report copied to clipboard')
    }).catch(() => {
      alert('复制失败，请手动选中上方错误文本')
    })
  }

  openIssue = () => {
    const title = encodeURIComponent(`[crash] ${this.state.error?.message.slice(0, 80) || 'unknown'}`)
    const body = encodeURIComponent(
      `请粘贴"复制错误"按钮复制的内容到这里：\n\n\n\n---\n环境：\n- 版本：\n- OS：${navigator.platform}\n- 复现步骤：`
    )
    const url = `https://github.com/zyswork/xianzhu-claw/issues/new?title=${title}&body=${body}`
    window.open(url, '_blank')
  }

  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 40, color: '#dc2626' }}>
          <h2>页面出错了 / Something went wrong</h2>
          <pre style={{ fontSize: 13, whiteSpace: 'pre-wrap', background: '#fef2f2', padding: 16, borderRadius: 8, maxHeight: 360, overflow: 'auto' }}>
            {this.state.error.message}
            {'\n\n'}
            {this.state.error.stack}
          </pre>
          <div style={{ marginTop: 16, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <button onClick={() => { this.setState({ error: null }); window.location.hash = '#/agents' }}
              style={{ padding: '8px 20px', cursor: 'pointer', borderRadius: 6, border: '1px solid #d1d5db', background: '#fff' }}>
              返回首页 / Back to Home
            </button>
            <button onClick={this.copyReport}
              style={{ padding: '8px 20px', cursor: 'pointer', borderRadius: 6, border: '1px solid #d1d5db', background: '#fff' }}>
              📋 复制错误 / Copy Report
            </button>
            <button onClick={this.openIssue}
              style={{ padding: '8px 20px', cursor: 'pointer', borderRadius: 6, border: 'none', background: '#dc2626', color: '#fff' }}>
              🐛 提交 Bug / Report Issue
            </button>
          </div>
          <p style={{ fontSize: 12, color: '#6b7280', marginTop: 12 }}>
            提交 Issue 前请先点"复制错误"，粘贴到模板的对应位置。不包含对话内容或 API Key。
          </p>
        </div>
      )
    }
    return this.props.children
  }
}

function ProtectedPage({ children }: { children: React.ReactNode }) {
  const { isLoggedIn } = useAuthStore()
  // 未登录 → 强制跳转登录页
  if (!isLoggedIn) {
    return <Navigate to="/login" replace />
  }
  return (
    <Layout>
      <ErrorBoundary>
        <Suspense fallback={<PageLoader />}>
          {children}
        </Suspense>
      </ErrorBoundary>
    </Layout>
  )
}

export default function App() {
  const { isConnected, retryCount } = useBackendConnection()
  const { t } = useI18n()
  const { hydrate, loadProfile } = useAuthStore()
  const { updateAvailable, updating, installUpdate, dismissUpdate } = useUpdater()
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null)

  // 启动时恢复登录状态
  useEffect(() => {
    hydrate()
  }, [hydrate])

  // 连接后加载个人资料
  useEffect(() => {
    if (isConnected) loadProfile()
  }, [isConnected, loadProfile])

  // 连接后检查是否需要首次设置
  useEffect(() => {
    if (!isConnected) return
    ;(async () => {
      try {
        const setupDone = await invoke<string | null>('get_setting', { key: 'setup_completed' })
        setNeedsSetup(!setupDone)
      } catch {
        setNeedsSetup(true)
      }
    })()
  }, [isConnected])

  if (!isConnected) {
    return (
      <SplashScreen
        message={`${t('common.loading')} (${retryCount})`}
        progress={Math.min((retryCount / 10) * 100, 95)}
      />
    )
  }

  // 等待检查结果
  if (needsSetup === null) {
    return <SplashScreen message={t('setup.stepEnvCheck') + '...'} progress={50} />
  }

  // 首次启动引导
  if (needsSetup) {
    return (
      <SetupPage onComplete={async () => {
        await invoke('set_setting', { key: 'setup_completed', value: 'true' }).catch((e) => console.warn('Setup flag save failed:', e))
        setNeedsSetup(false)
      }} />
    )
  }

  return (
    <HashRouter>
      <OfflineBanner />
      <CommandPalette />
      <ToastContainer />
      <ConfirmDialog />
      <ApprovalDialog />
      {updateAvailable && (
        <div style={{
          // z-index 比 OfflineBanner (9999) 高一级；若离线 banner 同时显示，
          // updater 浮在上方，但两条都可见（由各自 top 偏移控制避免重叠）
          position: 'fixed', top: 0, left: 0, right: 0, zIndex: 10000,
          background: 'linear-gradient(90deg, #238636, #1a7f37)', color: 'white',
          padding: '10px 20px', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 16,
          fontSize: 13, fontWeight: 500, boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
        }}>
          <span>v{updateAvailable.version} {t('update.available') || '新版本可用'}</span>
          {updateAvailable.notes && <span style={{ opacity: 0.8, maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{updateAvailable.notes}</span>}
          <button onClick={installUpdate} disabled={updating} style={{
            padding: '4px 16px', border: '1px solid rgba(255,255,255,0.4)', borderRadius: 6,
            background: 'rgba(255,255,255,0.15)', color: 'white', cursor: 'pointer', fontSize: 12, fontWeight: 600,
          }}>
            {updating ? t('update.installing') || '安装中...' : t('update.install') || '立即更新'}
          </button>
          <button onClick={dismissUpdate} style={{
            padding: '4px 8px', border: 'none', background: 'transparent', color: 'rgba(255,255,255,0.7)',
            cursor: 'pointer', fontSize: 16, lineHeight: 1,
          }}>×</button>
        </div>
      )}
      <Routes>
        <Route path="/login" element={<Suspense fallback={<PageLoader />}><LoginPage /></Suspense>} />
        <Route path="/" element={<Navigate to="/agents" replace />} />
        <Route path="/agents" element={<ProtectedPage><AgentListPage /></ProtectedPage>} />
        <Route path="/agents/new" element={<ProtectedPage><AgentCreatePage /></ProtectedPage>} />
        <Route path="/agents/:agentId" element={<ProtectedPage><AgentDetailPage /></ProtectedPage>} />
        <Route path="/dashboard" element={<ProtectedPage><Dashboard /></ProtectedPage>} />
        <Route path="/skills" element={<ProtectedPage><SkillsPage /></ProtectedPage>} />
        <Route path="/memory" element={<ProtectedPage><MemoryPage /></ProtectedPage>} />
        <Route path="/cron" element={<ProtectedPage><CronPage /></ProtectedPage>} />
        <Route path="/audit" element={<ProtectedPage><AuditLogPage /></ProtectedPage>} />
        <Route path="/token-monitoring" element={<ProtectedPage><TokenMonitoringPage /></ProtectedPage>} />
        <Route path="/doctor" element={<ProtectedPage><DoctorPage /></ProtectedPage>} />
        <Route path="/channels" element={<ProtectedPage><ChannelsPage /></ProtectedPage>} />
        <Route path="/plugins" element={<ProtectedPage><PluginsPage /></ProtectedPage>} />
        <Route path="/plaza" element={<ProtectedPage><PlazaPage /></ProtectedPage>} />
        <Route path="/group-chat" element={<ProtectedPage><GroupChatPage /></ProtectedPage>} />
        <Route path="/settings" element={<ProtectedPage><SettingsPage /></ProtectedPage>} />
        <Route path="/compare" element={<ProtectedPage><ModelComparePage /></ProtectedPage>} />
        <Route path="*" element={<Navigate to="/agents" replace />} />
      </Routes>
    </HashRouter>
  )
}
