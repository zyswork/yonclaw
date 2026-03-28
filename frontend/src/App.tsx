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
import Layout from './components/Layout'

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
  render() {
    if (this.state.error) {
      return (
        <div style={{ padding: 40, color: '#dc2626' }}>
          <h2>Something went wrong / 页面出错了</h2>
          <pre style={{ fontSize: 13, whiteSpace: 'pre-wrap', background: '#fef2f2', padding: 16, borderRadius: 8 }}>
            {this.state.error.message}
            {'\n\n'}
            {this.state.error.stack}
          </pre>
          <button onClick={() => { this.setState({ error: null }); window.location.hash = '#/agents' }}
            style={{ marginTop: 16, padding: '8px 20px', cursor: 'pointer' }}>
            Back to Home / 返回首页
          </button>
        </div>
      )
    }
    return this.props.children
  }
}

function ProtectedPage({ children }: { children: React.ReactNode }) {
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
  const { hydrate } = useAuthStore()
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null)

  // 启动时恢复登录状态
  useEffect(() => {
    hydrate()
  }, [hydrate])

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
      <ToastContainer />
      <ConfirmDialog />
      <ApprovalDialog />
      <Routes>
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
        <Route path="*" element={<Navigate to="/agents" replace />} />
      </Routes>
    </HashRouter>
  )
}
