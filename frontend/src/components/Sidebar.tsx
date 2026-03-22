/**
 * 侧边栏 — 深色风格（参考 PetClaw 左侧导航）
 * 支持响应式折叠：< 768px 自动折叠为图标模式
 */

import { useState, useEffect } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { useI18n } from '../i18n'

/** 判断当前视口是否为窄屏 */
function isNarrowViewport() {
  return window.matchMedia('(max-width: 768px)').matches
}

export default function Sidebar() {
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useI18n()
  const [collapsed, setCollapsed] = useState(isNarrowViewport)

  // 监听视口宽度变化，自动折叠/展开
  useEffect(() => {
    const mql = window.matchMedia('(max-width: 768px)')
    const handler = (e: MediaQueryListEvent) => setCollapsed(e.matches)
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [])

  const menuItems = [
    { icon: '\u{1F4AC}', label: t('sidebar.chat'), path: '/agents' },
    { icon: '\u{1F9E9}', label: t('sidebar.skills'), path: '/skills' },
    { icon: '\u23F0', label: t('sidebar.cron'), path: '/cron' },
    { icon: '\u{1F4E8}', label: t('sidebar.channels'), path: '/channels' },
    { icon: '\u{1F50C}', label: t('sidebar.plugins'), path: '/plugins' },
    { icon: '\u{1F4CA}', label: t('sidebar.dashboard'), path: '/dashboard' },
    { icon: '\u{1F9E0}', label: t('sidebar.memory'), path: '/memory' },
    { icon: '\u2699\uFE0F', label: t('sidebar.settings'), path: '/settings' },
  ]

  const sidebarWidth = collapsed ? '50px' : '200px'

  return (
    <aside style={{
      width: sidebarWidth,
      minWidth: sidebarWidth,
      backgroundColor: 'var(--sidebar-bg)',
      display: 'flex',
      flexDirection: 'column',
      color: 'var(--sidebar-text)',
      transition: 'width 0.2s ease, min-width 0.2s ease',
      overflow: 'hidden',
    }}>
      {/* 顶部：Logo + 折叠按钮 */}
      <div style={{ padding: collapsed ? '12px 0' : '20px 18px 16px', borderBottom: '1px solid var(--sidebar-border)' }}>
        {collapsed ? (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 }}>
            <button
              onClick={() => setCollapsed(false)}
              style={{
                background: 'none', border: 'none', color: 'rgba(255,255,255,0.6)',
                cursor: 'pointer', fontSize: 18, padding: 4, lineHeight: 1,
              }}
              title={t('sidebar.expand')}
            >
              ☰
            </button>
            <img src="/avatar-ai.png" alt="YonClaw" style={{ width: 28, height: 28, borderRadius: '50%' }} />
          </div>
        ) : (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <button
              onClick={() => setCollapsed(true)}
              style={{
                background: 'none', border: 'none', color: 'rgba(255,255,255,0.6)',
                cursor: 'pointer', fontSize: 16, padding: '2px 4px', lineHeight: 1,
                marginRight: 4,
              }}
              title={t('sidebar.collapse')}
            >
              ☰
            </button>
            <img src="/avatar-ai.png" alt="YonClaw" style={{ width: 32, height: 32, borderRadius: '50%' }} />
            <div>
              <div style={{ fontSize: '15px', fontWeight: 700, color: '#fff', letterSpacing: '-0.02em' }}>YonClaw</div>
              <div style={{ fontSize: '10px', color: 'rgba(255,255,255,0.4)' }}>AI Assistant</div>
            </div>
          </div>
        )}
      </div>

      {/* + 新建聊天 */}
      {!collapsed && (
        <div style={{ padding: '12px 14px 8px' }}>
          <button
            onClick={() => navigate('/agents/new')}
            style={{
              width: '100%', padding: '8px', borderRadius: 8,
              backgroundColor: 'rgba(255,255,255,0.08)', color: '#fff',
              border: '1px solid rgba(255,255,255,0.12)', cursor: 'pointer',
              fontSize: 13, fontWeight: 500,
            }}
          >
            {t('sidebar.newChat')}
          </button>
        </div>
      )}
      {collapsed && (
        <div style={{ padding: '8px 0', display: 'flex', justifyContent: 'center' }}>
          <button
            onClick={() => navigate('/agents/new')}
            style={{
              background: 'none', border: 'none', color: '#fff',
              cursor: 'pointer', fontSize: 18, padding: 4, lineHeight: 1,
            }}
            title={t('sidebar.newChat')}
          >
            +
          </button>
        </div>
      )}

      {/* 导航 */}
      <nav style={{ flex: 1, padding: '4px 0', overflowY: 'auto' }}>
        {menuItems.map((item) => {
          const isActive = location.pathname === item.path ||
            (item.path === '/agents' && location.pathname.startsWith('/agents'))
          return (
            <a
              key={item.path}
              href={item.path}
              onClick={(e) => { e.preventDefault(); navigate(item.path) }}
              title={collapsed ? item.label : undefined}
              style={{
                display: 'block',
                padding: collapsed ? '10px 0' : '9px 18px',
                textAlign: collapsed ? 'center' : 'left',
                color: isActive ? '#fff' : 'var(--sidebar-text)',
                textDecoration: 'none',
                fontSize: collapsed ? '16px' : '13px',
                backgroundColor: isActive ? 'var(--sidebar-active)' : 'transparent',
                fontWeight: isActive ? 600 : 400,
                borderRadius: 0,
                transition: 'all 0.12s ease',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
              }}
              onMouseEnter={(e) => {
                if (!isActive) (e.currentTarget as HTMLAnchorElement).style.backgroundColor = 'rgba(255,255,255,0.06)'
              }}
              onMouseLeave={(e) => {
                (e.currentTarget as HTMLAnchorElement).style.backgroundColor = isActive ? 'var(--sidebar-active)' : 'transparent'
              }}
            >
              {collapsed ? item.icon : <>{item.icon}  {item.label}</>}
            </a>
          )
        })}
      </nav>

      {/* 底部链接（展开时显示） */}
      {!collapsed && (
        <div style={{ padding: '8px 14px' }}>
          <a
            href="/audit"
            onClick={(e) => { e.preventDefault(); navigate('/audit') }}
            style={{ display: 'block', padding: '6px 4px', fontSize: 12, color: 'rgba(255,255,255,0.35)', textDecoration: 'none' }}
          >
            {'\u{1F4DD}'} {t('sidebar.audit')}
          </a>
          <a
            href="/token-monitoring"
            onClick={(e) => { e.preventDefault(); navigate('/token-monitoring') }}
            style={{ display: 'block', padding: '6px 4px', fontSize: 12, color: 'rgba(255,255,255,0.35)', textDecoration: 'none' }}
          >
            {'\u{1F4C8}'} {t('sidebar.tokenMonitor')}
          </a>
        </div>
      )}

      <div style={{
        padding: collapsed ? '10px 4px' : '10px 18px',
        borderTop: '1px solid var(--sidebar-border)',
        fontSize: '11px',
        color: 'rgba(255,255,255,0.25)',
        textAlign: collapsed ? 'center' : 'left',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
      }}>
        {collapsed ? 'v0.1' : 'YonClaw v0.1.0'}
      </div>
    </aside>
  )
}
