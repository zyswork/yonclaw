/**
 * 侧边栏 — Technolize 风格，深色毛玻璃
 * 分组导航 + 底部设置/主题/用户区域
 * 支持响应式折叠：< 768px 自动折叠为图标模式
 */

import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate, useLocation } from 'react-router-dom'
import { useI18n } from '../i18n'
import { useAuthStore } from '../store/authStore'
import { useSidebarStore } from '../store/sidebarStore'
import { useTheme, type Theme } from '../hooks/useTheme'
import {
  IconChat, IconGroup, IconSkills, IconCron, IconChannels,
  IconPlugins, IconPlaza, IconDashboard, IconMemory, IconSettings,
  IconAudit, IconTokens, IconDoctor, IconMenu, IconPlus,
  IconSun, IconMoon, IconMonitor, IconLogout,
} from './Icons'

/** 判断当前视口是否为窄屏 */
function isNarrowViewport() {
  return window.matchMedia('(max-width: 768px)').matches
}

interface NavItem {
  icon: React.ReactNode
  label: string
  path: string
  /** 快捷键编号（1~9），用于显示 ⌘N 提示 */
  shortcutNum?: number
}

export default function Sidebar() {
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useI18n()
  const { user, logout, nickname, avatarUrl } = useAuthStore()
  const { theme, setTheme } = useTheme()
  const { collapsed, setCollapsed } = useSidebarStore()
  const [showAbout, setShowAbout] = useState(false)

  // 监听视口宽度变化，自动折叠/展开
  useEffect(() => {
    const mql = window.matchMedia('(max-width: 768px)')
    const handler = (e: MediaQueryListEvent) => setCollapsed(e.matches)
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [])

  // 主功能导航（shortcutNum 对应 Cmd+N 快捷键）
  const mainNav: NavItem[] = [
    { icon: <IconChat size={18} />, label: t('sidebar.chat'), path: '/agents', shortcutNum: 1 },
    { icon: <IconGroup size={18} />, label: t('sidebar.groupChat'), path: '/group-chat', shortcutNum: 2 },
    { icon: <IconSkills size={18} />, label: t('sidebar.skills'), path: '/skills', shortcutNum: 3 },
    { icon: <IconCron size={18} />, label: t('sidebar.cron'), path: '/cron', shortcutNum: 4 },
    { icon: <IconChannels size={18} />, label: t('sidebar.channels'), path: '/channels', shortcutNum: 5 },
  ]

  // 管理导航
  const manageNav: NavItem[] = [
    { icon: <IconDashboard size={18} />, label: t('sidebar.dashboard'), path: '/dashboard', shortcutNum: 6 },
    { icon: <IconMemory size={18} />, label: t('sidebar.memory'), path: '/memory', shortcutNum: 7 },
    { icon: <IconPlugins size={18} />, label: t('sidebar.plugins'), path: '/plugins', shortcutNum: 8 },
    { icon: <IconPlaza size={18} />, label: t('sidebar.plaza'), path: '/plaza', shortcutNum: 9 },
  ]

  // 主题切换选项
  const themeOptions: { key: Theme; icon: React.ReactNode; label: string }[] = [
    { key: 'dark', icon: <IconMoon size={12} />, label: 'Dark' },
    { key: 'light', icon: <IconSun size={12} />, label: 'Light' },
    { key: 'system', icon: <IconMonitor size={12} />, label: 'Auto' },
  ]

  const sidebarWidth = collapsed ? '54px' : '210px'

  /** 渲染单个导航项 */
  const renderNavItem = (item: NavItem) => {
    const isActive = location.pathname === item.path ||
      (item.path === '/agents' && location.pathname.startsWith('/agents'))

    const shortcutHint = item.shortcutNum ? ` (${'\u2318'}${item.shortcutNum})` : ''
    const tooltipText = collapsed ? item.label + shortcutHint : undefined

    const className = [
      'sidebar-nav-item',
      isActive && 'sidebar-nav-item--active',
      collapsed && 'sidebar-nav-item--collapsed',
      collapsed && 'sidebar-tooltip',
    ].filter(Boolean).join(' ')

    return (
      <a
        key={item.path}
        href={item.path}
        onClick={(e) => { e.preventDefault(); navigate(item.path) }}
        className={className}
        title={item.shortcutNum ? `${item.label} (${'\u2318'}${item.shortcutNum})` : item.label}
        data-tooltip={tooltipText}
      >
        {collapsed ? (
          item.icon
        ) : (
          <>
            <span className="sidebar-nav-icon">{item.icon}</span>
            {item.label}
          </>
        )}
      </a>
    )
  }

  /** 渲染导航分组 */
  const renderNavGroup = (title: string, items: NavItem[]) => (
    <div key={title}>
      {!collapsed && <div className="sidebar-section-title">{title}</div>}
      {items.map(renderNavItem)}
    </div>
  )

  return (
    <>
    <aside
      className="sidebar"
      style={{ width: sidebarWidth, minWidth: sidebarWidth, paddingTop: 34 }}
    >
      {/* 顶部：Logo + 折叠按钮（34px padding 避开 macOS 红绿灯） */}
      <div className={collapsed ? 'sidebar-header--collapsed' : 'sidebar-header'}>
        {collapsed ? (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 }}>
            <button
              className="sidebar-toggle"
              onClick={() => setCollapsed(false)}
              title={t('sidebar.expand')}
            >
              <IconMenu size={16} />
            </button>
            <img
              src="/avatar-ai.png"
              alt="XianZhuClaw"
              style={{ width: 28, height: 28, borderRadius: '50%', opacity: 0.8 }}
            />
          </div>
        ) : (
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <button
              className="sidebar-toggle"
              onClick={() => setCollapsed(true)}
              title={t('sidebar.collapse')}
            >
              <IconMenu size={16} />
            </button>
            <img
              src="/avatar-ai.png"
              alt="XianZhuClaw"
              style={{ width: 32, height: 32, borderRadius: '50%' }}
            />
            <div>
              <div className="sidebar-brand">XianZhu</div>
              <div className="sidebar-subtitle">AI 智能助手</div>
            </div>
          </div>
        )}
      </div>

      {/* + 新建聊天 */}
      <div style={{ padding: collapsed ? '8px 6px' : '12px 14px 8px' }}>
        {collapsed ? (
          <button
            className="sidebar-new-btn--icon"
            onClick={() => navigate('/agents/new')}
            title={`${t('sidebar.newChat')} (\u2318N)`}
          >
            <IconPlus size={16} />
          </button>
        ) : (
          <button
            className="sidebar-new-btn"
            onClick={() => navigate('/agents/new')}
          >
            <IconPlus size={15} />
            {t('sidebar.newChat')}
          </button>
        )}
      </div>

      {/* 分组导航 */}
      <nav style={{ flex: 1, padding: '4px 0', overflowY: 'auto' }}>
        {renderNavGroup(t('sidebar.sectionMain'), mainNav)}
        {renderNavGroup(t('sidebar.sectionManage'), manageNav)}
      </nav>

      {/* ── 底部区域 ── */}
      <div className="sidebar-bottom">
        {/* 设置链接 (⌘,) */}
        {renderNavItem({ icon: <IconSettings size={18} />, label: t('sidebar.settings'), path: '/settings' })}

        {/* 主题切换（展开时显示） */}
        {!collapsed && (
          <div className="sidebar-theme-toggle">
            {themeOptions.map(opt => (
              <button
                key={opt.key}
                className={theme === opt.key ? 'active' : ''}
                onClick={() => setTheme(opt.key)}
                title={opt.label}
              >
                {opt.icon}
                <span>{opt.label}</span>
              </button>
            ))}
          </div>
        )}

        {/* 小字链接：审计 / 监控 / 诊断 */}
        {!collapsed ? (
          <div className="sidebar-links">
            <a href="/audit" onClick={(e) => { e.preventDefault(); navigate('/audit') }}>
              {t('sidebar.audit')}
            </a>
            <span className="sidebar-links-dot">&middot;</span>
            <a href="/token-monitoring" onClick={(e) => { e.preventDefault(); navigate('/token-monitoring') }}>
              {t('sidebar.tokenMonitor')}
            </a>
            <span className="sidebar-links-dot">&middot;</span>
            <a href="/doctor" onClick={(e) => { e.preventDefault(); navigate('/doctor') }}>
              {t('sidebar.doctor')}
            </a>
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2, padding: '4px 0' }}>
            <a
              href="/audit"
              onClick={(e) => { e.preventDefault(); navigate('/audit') }}
              className="sidebar-nav-item sidebar-nav-item--collapsed sidebar-tooltip"
              data-tooltip={t('sidebar.audit')}
            >
              <IconAudit size={16} />
            </a>
            <a
              href="/token-monitoring"
              onClick={(e) => { e.preventDefault(); navigate('/token-monitoring') }}
              className="sidebar-nav-item sidebar-nav-item--collapsed sidebar-tooltip"
              data-tooltip={t('sidebar.tokenMonitor')}
            >
              <IconTokens size={16} />
            </a>
            <a
              href="/doctor"
              onClick={(e) => { e.preventDefault(); navigate('/doctor') }}
              className="sidebar-nav-item sidebar-nav-item--collapsed sidebar-tooltip"
              data-tooltip={t('sidebar.doctor')}
            >
              <IconDoctor size={16} />
            </a>
          </div>
        )}

        {/* 分隔线 */}
        <div className="sidebar-divider" />

        {/* 用户信息 */}
        {!collapsed && (user || nickname || avatarUrl) && (
          <div
            className="sidebar-user"
            style={{ cursor: 'pointer' }}
            onClick={() => navigate('/settings?section=profile')}
          >
            <div className="sidebar-user-avatar" style={{ overflow: 'hidden' }}>
              {avatarUrl ? (
                <img src={avatarUrl} alt="avatar" style={{ width: '100%', height: '100%', objectFit: 'cover', borderRadius: '50%' }} />
              ) : (
                ((nickname || user?.name || user?.email || 'U')[0].toUpperCase())
              )}
            </div>
            <div className="sidebar-user-info">
              <div className="sidebar-user-name">{nickname || user?.name || user?.email || 'User'}</div>
              {user?.email && <div className="sidebar-user-email">{user.email}</div>}
            </div>
          </div>
        )}
        {collapsed && (user || nickname || avatarUrl) && (
          <div
            style={{ display: 'flex', justifyContent: 'center', padding: '6px 0', cursor: 'pointer' }}
            onClick={() => navigate('/settings?section=profile')}
          >
            <div
              className="sidebar-user-avatar sidebar-tooltip"
              data-tooltip={nickname || user?.email || 'User'}
              style={{ overflow: 'hidden' }}
            >
              {avatarUrl ? (
                <img src={avatarUrl} alt="avatar" style={{ width: '100%', height: '100%', objectFit: 'cover', borderRadius: '50%' }} />
              ) : (
                ((nickname || user?.name || user?.email || 'U')[0].toUpperCase())
              )}
            </div>
          </div>
        )}

        {/* 退出按钮 */}
        {user && !collapsed && (
          <button className="sidebar-logout-btn" onClick={() => { logout(); navigate('/login') }}>
            <IconLogout size={14} />
            {t('sidebar.logout')}
          </button>
        )}
        {user && collapsed && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: '4px 0' }}>
            <button
              className="sidebar-logout-btn--icon sidebar-tooltip"
              data-tooltip={t('sidebar.logout')}
              onClick={() => { logout(); navigate('/login') }}
            >
              <IconLogout size={14} />
            </button>
          </div>
        )}

        {/* 未登录时：显示登录按钮 */}
        {!user && !collapsed && (
          <button className="sidebar-logout-btn" style={{ color: 'var(--accent)' }} onClick={() => navigate('/login')}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/>
            </svg>
            {t('login.loginBtn')}
          </button>
        )}
        {!user && collapsed && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: '4px 0' }}>
            <button
              className="sidebar-logout-btn--icon sidebar-tooltip"
              data-tooltip={t('login.loginBtn')}
              style={{ color: 'var(--accent)' }}
              onClick={() => navigate('/login')}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/>
              </svg>
            </button>
          </div>
        )}

        {/* 关于 XianZhuClaw — 点击打开关于弹窗 */}
        <button
          onClick={() => setShowAbout(true)}
          style={{
            width: collapsed ? 'auto' : 'calc(100% - 28px)',
            margin: collapsed ? '4px auto' : '4px 14px 8px',
            padding: collapsed ? '6px' : '8px 12px',
            display: 'flex', alignItems: 'center', justifyContent: collapsed ? 'center' : 'space-between',
            gap: 8, border: '1px solid var(--border-subtle)', borderRadius: 8,
            background: 'transparent', color: 'var(--text-muted)', cursor: 'pointer',
            fontSize: 12, transition: 'all 0.15s',
          }}
          onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-elevated)'; e.currentTarget.style.color = 'var(--text-primary)' }}
          onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-muted)' }}
          title={t('sidebar.about') || '关于 XianZhuClaw'}
        >
          {collapsed ? (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
          ) : (
            <>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
                {t('sidebar.about') || '关于'} XianZhuClaw
              </span>
              <span style={{ fontSize: 11, opacity: 0.6 }}>v0.2.0</span>
            </>
          )}
        </button>
      </div>

    </aside>
    {showAbout && createPortal(
      <AboutDialog onClose={() => setShowAbout(false)} />,
      document.body
    )}
  </>
  )
}

/** 关于弹窗 — 版本信息 + 检查更新 */
function AboutDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n()
  const [checking, setChecking] = useState(false)
  const [updateResult, setUpdateResult] = useState<string | null>(null)
  const [updating, setUpdating] = useState(false)

  const handleCheckUpdate = async () => {
    setChecking(true)
    setUpdateResult(null)
    try {
      const { checkUpdate } = await import('@tauri-apps/api/updater')
      const { shouldUpdate, manifest } = await checkUpdate()
      if (shouldUpdate && manifest) {
        setUpdateResult(`v${manifest.version} ${t('update.available') || '可用'}`)
      } else {
        setUpdateResult(t('about.upToDate') || '已是最新版本')
      }
    } catch {
      setUpdateResult(t('about.checkFailed') || '检查失败，请稍后重试')
    }
    setChecking(false)
  }

  const handleInstallUpdate = async () => {
    setUpdating(true)
    try {
      const { installUpdate } = await import('@tauri-apps/api/updater')
      const { relaunch } = await import('@tauri-apps/api/process')
      await installUpdate()
      await relaunch()
    } catch {
      setUpdating(false)
    }
  }

  return (
    <div onClick={onClose} style={{
      position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)', display: 'flex',
      alignItems: 'center', justifyContent: 'center', zIndex: 9999,
    }}>
      <div onClick={e => e.stopPropagation()} style={{
        background: 'var(--bg-elevated)', borderRadius: 16, padding: 32, width: 360,
        border: '1px solid var(--border-subtle)', boxShadow: '0 20px 60px rgba(0,0,0,0.3)',
        textAlign: 'center',
      }}>
        <img src="/avatar-ai.png" alt="XianZhuClaw" style={{ width: 64, height: 64, borderRadius: '50%', marginBottom: 12 }} />
        <h2 style={{ margin: '0 0 4px', fontSize: 20, fontWeight: 700 }}>XianZhuClaw 衔烛Claw</h2>
        <p style={{ margin: '0 0 4px', fontSize: 13, color: 'var(--text-muted)' }}>
          {t('about.tagline') || 'AI 原生桌面助手，多智能体协作'}
        </p>
        <p style={{ margin: '0 0 16px', fontSize: 14, fontWeight: 600, color: 'var(--accent)' }}>v0.2.0</p>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 16 }}>
          <button onClick={handleCheckUpdate} disabled={checking || updating} style={{
            padding: '10px 0', border: '1px solid var(--border-subtle)', borderRadius: 8,
            background: 'var(--bg-base)', color: 'var(--text-primary)', cursor: 'pointer',
            fontSize: 13, fontWeight: 500,
          }}>
            {checking ? '...' : updating ? (t('update.installing') || '安装中...') : (t('about.checkUpdate') || '检查更新')}
          </button>
          {updateResult && (
            <div style={{ fontSize: 12, color: updateResult.includes('可用') || updateResult.includes('available') ? 'var(--accent)' : 'var(--text-muted)' }}>
              {updateResult}
              {(updateResult.includes('可用') || updateResult.includes('available')) && (
                <button onClick={handleInstallUpdate} disabled={updating} style={{
                  marginLeft: 8, padding: '2px 12px', border: 'none', borderRadius: 4,
                  background: 'var(--accent)', color: 'white', cursor: 'pointer', fontSize: 11,
                }}>
                  {t('update.install') || '立即更新'}
                </button>
              )}
            </div>
          )}
        </div>

        {/* 意见反馈 */}
        <a
          href="https://github.com/zyswork/xianzhu-claw/issues/new"
          target="_blank"
          rel="noopener"
          style={{
            display: 'block', padding: '10px 0', border: '1px solid var(--border-subtle)',
            borderRadius: 8, background: 'var(--bg-base)', color: 'var(--text-primary)',
            cursor: 'pointer', fontSize: 13, fontWeight: 500, textDecoration: 'none',
            textAlign: 'center', marginBottom: 16,
          }}
        >
          {t('about.feedback') || '意见反馈'}
        </a>

        <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.8 }}>
          <div style={{ marginBottom: 6 }}>
            <span style={{ fontWeight: 500, color: 'var(--text-secondary)' }}>{t('about.author') || '作者'}</span>
            <span style={{ marginLeft: 8 }}>张永顺</span>
          </div>
          <div style={{ marginBottom: 6 }}>
            <span style={{ fontWeight: 500, color: 'var(--text-secondary)' }}>{t('about.email') || '邮箱'}</span>
            <a href="mailto:zys_work@outlook.com" style={{ marginLeft: 8, color: 'var(--accent)', textDecoration: 'none' }}>
              zys_work@outlook.com
            </a>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8, marginTop: 8 }}>
            <a href="https://github.com/zyswork/xianzhu-claw" target="_blank" rel="noopener"
              style={{ color: 'var(--accent)', textDecoration: 'none' }}>GitHub</a>
            <span>·</span>
            <span>MIT License</span>
            <span>·</span>
            <span>Rust + React + Tauri</span>
          </div>
        </div>

        <button onClick={onClose} style={{
          marginTop: 16, padding: '8px 32px', border: '1px solid var(--border-subtle)',
          borderRadius: 8, background: 'transparent', color: 'var(--text-secondary)',
          cursor: 'pointer', fontSize: 13,
        }}>
          {t('common.close') || '关闭'}
        </button>
      </div>
    </div>
  )
}
