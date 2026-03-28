/**
 * 侧边栏 — Technolize 风格，深色毛玻璃
 * 分组导航 + 底部设置/主题/用户区域
 * 支持响应式折叠：< 768px 自动折叠为图标模式
 */

import { useState, useEffect } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useI18n } from '../i18n'
import { useAuthStore } from '../store/authStore'
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
}

export default function Sidebar() {
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useI18n()
  const { user, logout } = useAuthStore()
  const { theme, setTheme } = useTheme()
  const [collapsed, setCollapsed] = useState(isNarrowViewport)

  // 监听视口宽度变化，自动折叠/展开
  useEffect(() => {
    const mql = window.matchMedia('(max-width: 768px)')
    const handler = (e: MediaQueryListEvent) => setCollapsed(e.matches)
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [])

  // 主功能导航
  const mainNav: NavItem[] = [
    { icon: <IconChat size={18} />, label: t('sidebar.chat'), path: '/agents' },
    { icon: <IconGroup size={18} />, label: t('sidebar.groupChat'), path: '/group-chat' },
    { icon: <IconSkills size={18} />, label: t('sidebar.skills'), path: '/skills' },
    { icon: <IconCron size={18} />, label: t('sidebar.cron'), path: '/cron' },
    { icon: <IconChannels size={18} />, label: t('sidebar.channels'), path: '/channels' },
  ]

  // 管理导航
  const manageNav: NavItem[] = [
    { icon: <IconDashboard size={18} />, label: t('sidebar.dashboard'), path: '/dashboard' },
    { icon: <IconMemory size={18} />, label: t('sidebar.memory'), path: '/memory' },
    { icon: <IconPlugins size={18} />, label: t('sidebar.plugins'), path: '/plugins' },
    { icon: <IconPlaza size={18} />, label: t('sidebar.plaza'), path: '/plaza' },
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
        data-tooltip={collapsed ? item.label : undefined}
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
    <aside
      className="sidebar"
      style={{ width: sidebarWidth, minWidth: sidebarWidth }}
    >
      {/* 顶部：Logo + 折叠按钮 */}
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
              alt="XianZhu"
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
              alt="XianZhu"
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
            title={t('sidebar.newChat')}
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
        {/* 设置链接 */}
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
        {user && !collapsed && (
          <div className="sidebar-user">
            <div className="sidebar-user-avatar">
              {(user.name || user.email)[0].toUpperCase()}
            </div>
            <div className="sidebar-user-info">
              {user.name && <div className="sidebar-user-name">{user.name}</div>}
              <div className="sidebar-user-email">{user.email}</div>
            </div>
          </div>
        )}
        {user && collapsed && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: '6px 0' }}>
            <div
              className="sidebar-user-avatar sidebar-tooltip"
              data-tooltip={user.email}
            >
              {(user.name || user.email)[0].toUpperCase()}
            </div>
          </div>
        )}

        {/* 退出按钮 */}
        {user && !collapsed && (
          <button className="sidebar-logout-btn" onClick={logout}>
            <IconLogout size={14} />
            {t('sidebar.logout')}
          </button>
        )}
        {user && collapsed && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: '4px 0' }}>
            <button
              className="sidebar-logout-btn--icon sidebar-tooltip"
              data-tooltip={t('sidebar.logout')}
              onClick={logout}
            >
              <IconLogout size={14} />
            </button>
          </div>
        )}

        {/* 版本号 */}
        <div className={collapsed ? 'sidebar-version sidebar-version--collapsed' : 'sidebar-version'}>
          {collapsed ? 'v0.1' : 'v0.1.0'}
        </div>
      </div>
    </aside>
  )
}
