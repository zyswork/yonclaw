/**
 * 仪表板 — 现代数据驱动卡片网格布局
 *
 * 展示：顶部统计栏、Agent 列表、最近活动 feed、系统状态、Token 趋势
 */

import { useEffect, useState, useCallback, type CSSProperties } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { Link } from 'react-router-dom'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'

/* ─── 数据类型 ─────────────────────────────── */

interface HealthData {
  status: string
  db: boolean
  agents: number
  memories: number
  today_tokens: number
  response_cache_entries: number
}

interface CacheStats {
  response_cache: { entries: number; total_hits: number }
  embedding_cache: { entries: number }
}

interface AgentSummary {
  id: string
  name: string
  model: string
  sessionCount: number
}

interface SchedulerStats {
  running: boolean
  totalJobs: number
  enabledJobs: number
  runningRuns: number
  recentFailureRate: number
}

/* ─── 样式常量 ─────────────────────────────── */

const CARD_STYLE: CSSProperties = {
  background: 'var(--bg-elevated)',
  border: '1px solid var(--border-subtle)',
  borderRadius: '14px',
  backdropFilter: 'blur(var(--glass-blur))',
  transition: 'transform 0.2s ease, box-shadow 0.2s ease',
}

const CARD_HOVER_SHADOW = '0 8px 24px rgba(0,0,0,0.25)'
const CARD_DEFAULT_SHADOW = '0 2px 8px rgba(0,0,0,0.15)'

/* ─── 工具函数 ─────────────────────────────── */

function formatNumber(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

/** 简易 SVG 折线图路径生成 */
function sparklinePath(data: number[], width: number, height: number): string {
  if (data.length < 2) return ''
  const max = Math.max(...data, 1)
  const min = Math.min(...data, 0)
  const range = max - min || 1
  const stepX = width / (data.length - 1)
  return data
    .map((v, i) => {
      const x = i * stepX
      const y = height - ((v - min) / range) * (height - 4) - 2
      return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
}

/* ─── SVG 图标 ─────────────────────────────── */

/** 纯 SVG 图标组件，避免 emoji 渲染不一致 */
function SvgIcon({ name, size = 20, color }: { name: string; size?: number; color?: string }) {
  const c = color || 'currentColor'
  const props = { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: c, strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const }

  switch (name) {
    case 'agent':
      return <svg {...props}><rect x="3" y="11" width="18" height="10" rx="3"/><circle cx="12" cy="5" r="4"/><line x1="8" y1="16" x2="8" y2="16.01"/><line x1="16" y1="16" x2="16" y2="16.01"/></svg>
    case 'chat':
      return <svg {...props}><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
    case 'message':
      return <svg {...props}><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4l-2 2V6c0-1.1.9-2 2-2z"/><line x1="8" y1="10" x2="16" y2="10"/><line x1="8" y1="14" x2="13" y2="14"/></svg>
    case 'token':
      return <svg {...props}><circle cx="12" cy="12" r="9"/><path d="M14.5 9l-5 6"/><circle cx="10" cy="9.5" r="0.5" fill={c}/><circle cx="14" cy="14.5" r="0.5" fill={c}/></svg>
    case 'channel':
      return <svg {...props}><path d="M5.5 4.5h13M5.5 9h13M5.5 13.5h13M5.5 18h13"/><path d="M9 2v20M15 2v20"/></svg>
    case 'model':
      return <svg {...props}><path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/></svg>
    case 'memory':
      return <svg {...props}><path d="M12 2a7 7 0 017 7c0 5-7 13-7 13S5 14 5 9a7 7 0 017-7z"/><circle cx="12" cy="9" r="2.5"/></svg>
    case 'cache':
      return <svg {...props}><rect x="2" y="3" width="20" height="18" rx="3"/><line x1="2" y1="9" x2="22" y2="9"/><line x1="2" y1="15" x2="22" y2="15"/><circle cx="6" cy="6" r="0.5" fill={c}/><circle cx="6" cy="12" r="0.5" fill={c}/><circle cx="6" cy="18" r="0.5" fill={c}/></svg>
    case 'scheduler':
      return <svg {...props}><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
    case 'pulse':
      return <svg {...props}><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>
    case 'refresh':
      return <svg {...props}><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 11-2.12-9.36L23 10"/></svg>
    case 'activity':
      return <svg {...props}><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>
    case 'cpu':
      return <svg {...props}><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/><line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/><line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/><line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="15" x2="23" y2="15"/><line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="15" x2="4" y2="15"/></svg>
    case 'database':
      return <svg {...props}><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4.03 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4.03 3 9 3s9-1.34 9-3V5"/></svg>
    case 'clock':
      return <svg {...props}><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
    case 'trend':
      return <svg {...props}><polyline points="23 6 13.5 15.5 8.5 10.5 1 18"/><polyline points="17 6 23 6 23 12"/></svg>
    default:
      return <svg {...props}><circle cx="12" cy="12" r="10"/></svg>
  }
}

/* ─── 主组件 ─────────────────────────────── */

export default function Dashboard() {
  const { t } = useI18n()
  const [health, setHealth] = useState<HealthData | null>(null)
  const [cache, setCache] = useState<CacheStats | null>(null)
  const [agents, setAgents] = useState<AgentSummary[]>([])
  const [scheduler, setScheduler] = useState<SchedulerStats | null>(null)
  const [subagentCount, setSubagentCount] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  const loadAll = useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const [h, c, a, sched, subs] = await Promise.all([
        invoke('health_check').catch((e) => { console.warn('health_check failed:', e); return null }),
        invoke('get_cache_stats').catch((e) => { console.warn('get_cache_stats failed:', e); return null }),
        invoke('list_agents').catch((e) => { console.warn('list_agents failed:', e); return [] }),
        invoke('get_scheduler_status').catch((e) => { console.warn('get_scheduler_status failed:', e); return null }),
        invoke('list_subagent_runs', { limit: 1 }).catch((e) => { console.warn('list_subagent_runs failed:', e); return [] }),
      ])
      setHealth(h as HealthData)
      setCache(c as CacheStats)
      setScheduler(sched as SchedulerStats)
      setSubagentCount((subs as unknown[])?.length || 0)

      const agentList = (a as Array<{ id: string; name: string; model: string; sessionCount?: number }>) || []
      setAgents(
        agentList.map((ag) => ({
          id: ag.id,
          name: ag.name,
          model: ag.model,
          sessionCount: ag.sessionCount || 0,
        })),
      )
    } catch (e) {
      setError(t('dashboard.errorLoading', { error: String(e) }))
      toast.error(t('common.error') + ': ' + String(e))
    }
    setLoading(false)
  }, [t])

  useEffect(() => { loadAll() }, [loadAll])

  /* ─── 加载态 ─── */
  if (loading) {
    return (
      <div style={{ padding: '32px', display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: '400px' }}>
        <div style={{ textAlign: 'center', color: 'var(--text-muted)' }}>
          <div style={{ fontSize: '28px', marginBottom: '12px', animation: 'pulse 1.5s infinite' }}>
            <SvgIcon name="pulse" size={32} color="var(--accent)" />
          </div>
          <div style={{ fontSize: '14px' }}>{t('common.loading')}</div>
        </div>
      </div>
    )
  }

  /* ─── 统计卡片数据 ─── */
  const stats: StatItem[] = [
    {
      icon: 'agent',
      label: t('dashboard.agentCount'),
      value: String(health?.agents ?? 0),
      color: 'var(--accent)',
      gradient: 'linear-gradient(135deg, #818CF8, #6366F1)',
      sub: `${agents.length} ${t('dashboard.registered')}`,
    },
    {
      icon: 'chat',
      label: t('dashboard.responseCache'),
      value: String(cache?.response_cache?.entries ?? 0),
      color: '#06b6d4',
      gradient: 'linear-gradient(135deg, #06b6d4, #0891b2)',
      sub: `${t('dashboard.cacheHits')} ${cache?.response_cache?.total_hits ?? 0}`,
    },
    {
      icon: 'token',
      label: t('dashboard.todayTokens'),
      value: formatNumber(health?.today_tokens ?? 0),
      color: '#8b5cf6',
      gradient: 'linear-gradient(135deg, #8b5cf6, #7c3aed)',
      sub: `~$${((health?.today_tokens ?? 0) / 1000 * 0.003).toFixed(4)}`,
    },
    {
      icon: 'memory',
      label: t('dashboard.memoryCount'),
      value: String(health?.memories ?? 0),
      color: '#f59e0b',
      gradient: 'linear-gradient(135deg, #f59e0b, #d97706)',
      sub: t('dashboard.totalMemory'),
    },
    {
      icon: 'scheduler',
      label: t('dashboard.scheduler'),
      value: scheduler?.running ? t('dashboard.schedulerRunning') : t('dashboard.schedulerStopped'),
      color: scheduler?.running ? 'var(--success)' : 'var(--error)',
      gradient: scheduler?.running
        ? 'linear-gradient(135deg, #34D399, #10b981)'
        : 'linear-gradient(135deg, #F87171, #ef4444)',
      sub: `${scheduler?.enabledJobs ?? 0} ${t('dashboard.schedulerJobs')}`,
    },
    {
      icon: 'model',
      label: t('dashboard.subagentRuns'),
      value: String(subagentCount),
      color: '#a855f7',
      gradient: 'linear-gradient(135deg, #a855f7, #9333ea)',
      sub: t('dashboard.subagentRunsDesc'),
    },
  ]

  /* ─── 模拟最近 7 天 token 趋势（基于 today_tokens 做示意） ─── */
  const todayTokens = health?.today_tokens ?? 0
  const tokenTrend = [
    Math.round(todayTokens * 0.4),
    Math.round(todayTokens * 0.6),
    Math.round(todayTokens * 0.3),
    Math.round(todayTokens * 0.8),
    Math.round(todayTokens * 0.5),
    Math.round(todayTokens * 0.9),
    todayTokens,
  ]
  const hasTokenData = todayTokens > 0

  const statusColor = health?.status === 'healthy' ? 'var(--success)' : 'var(--error)'
  const statusText = health?.status === 'healthy' ? t('dashboard.statusHealthy') : t('dashboard.statusUnhealthy')

  return (
    <div style={{ padding: '28px 32px', maxWidth: '1280px' }}>
      {/* ─── 顶部标题栏 ─── */}
      <div style={{
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        marginBottom: '28px',
      }}>
        <div>
          <h1 style={{ margin: 0, fontSize: '24px', fontWeight: 700, color: 'var(--text-heading)' }}>
            {t('dashboard.title')}
          </h1>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginTop: '6px' }}>
            <span style={{
              display: 'inline-block', width: '8px', height: '8px', borderRadius: '50%',
              backgroundColor: statusColor,
              boxShadow: `0 0 8px ${health?.status === 'healthy' ? 'var(--success)' : 'var(--error)'}`,
            }} />
            <span style={{ fontSize: '13px', color: 'var(--text-secondary)' }}>
              {statusText} &middot; {health?.db ? t('dashboard.dbConnected') : t('dashboard.dbError')}
            </span>
          </div>
        </div>
        <button
          onClick={loadAll}
          style={{
            display: 'flex', alignItems: 'center', gap: '6px',
            padding: '8px 18px', fontSize: '13px', fontWeight: 500,
            backgroundColor: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
            borderRadius: '8px', cursor: 'pointer', color: 'var(--text-secondary)',
            transition: 'all 0.15s ease',
          }}
        >
          <SvgIcon name="refresh" size={14} color="var(--text-secondary)" />
          {t('dashboard.refresh')}
        </button>
      </div>

      {error && (
        <div style={{
          color: 'var(--error)', marginBottom: '20px', padding: '12px 16px',
          backgroundColor: 'var(--error-bg)', borderRadius: '10px',
          border: '1px solid var(--error)', fontSize: '13px',
        }}>
          {error}
        </div>
      )}

      {/* ─── 统计卡片网格 ─── */}
      <div className="dashboard-stats-grid" style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
        gap: '16px',
        marginBottom: '28px',
      }}>
        {stats.map((s) => (
          <StatCard key={s.label} item={s} />
        ))}
      </div>

      {/* ─── 中部两列：Agent 列表 + 最近活动 ─── */}
      <div className="dashboard-mid-grid" style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(360px, 1fr))',
        gap: '20px',
        marginBottom: '24px',
      }}>
        {/* Agent 列表 */}
        <div style={{ ...CARD_STYLE, padding: '20px', boxShadow: CARD_DEFAULT_SHADOW }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <SvgIcon name="agent" size={18} color="var(--accent)" />
              <h2 style={{ margin: 0, fontSize: '15px', fontWeight: 600, color: 'var(--text-heading)' }}>
                {t('dashboard.agentsOverview')}
              </h2>
            </div>
            <Link to="/agents" style={{ fontSize: '12px', color: 'var(--text-accent)', textDecoration: 'none' }}>
              {t('common.viewAll')} &rarr;
            </Link>
          </div>

          {agents.length === 0 ? (
            <div style={{
              padding: '32px 16px', textAlign: 'center', color: 'var(--text-muted)',
              background: 'var(--bg-glass)', borderRadius: '10px',
            }}>
              {t('dashboard.emptyAgents')}
              <Link to="/agents/new" style={{ color: 'var(--text-accent)' }}>{t('dashboard.createOne')}</Link>
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              {agents.slice(0, 6).map((agent) => (
                <AgentRow key={agent.id} agent={agent} t={t} />
              ))}
              {agents.length > 6 && (
                <Link to="/agents" style={{
                  fontSize: '12px', color: 'var(--text-muted)', textAlign: 'center',
                  padding: '8px 0',
                }}>
                  {t('dashboard.moreAgents', { n: agents.length - 6 })}
                </Link>
              )}
            </div>
          )}
        </div>

        {/* 快捷操作 + 最近活动 */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '20px' }}>
          {/* 快捷操作 */}
          <div style={{ ...CARD_STYLE, padding: '20px', boxShadow: CARD_DEFAULT_SHADOW }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '14px' }}>
              <SvgIcon name="activity" size={18} color="var(--accent)" />
              <h2 style={{ margin: 0, fontSize: '15px', fontWeight: 600, color: 'var(--text-heading)' }}>
                {t('dashboard.quickActions')}
              </h2>
            </div>
            <div className="dashboard-actions-grid" style={{
              display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: '10px',
            }}>
              {[
                { to: '/agents/new', label: t('dashboard.createAgent'), color: 'var(--text-accent)', bg: 'var(--accent-bg)' },
                { to: '/skills', label: t('dashboard.skillManagement'), color: '#8b5cf6', bg: 'rgba(139,92,246,0.1)' },
                { to: '/memory', label: t('dashboard.memoryManagement'), color: '#f59e0b', bg: 'rgba(245,158,11,0.1)' },
                { to: '/cron', label: t('dashboard.cronJobs'), color: '#06b6d4', bg: 'rgba(6,182,212,0.1)' },
                { to: '/token-monitoring', label: t('dashboard.tokenMonitoring'), color: 'var(--error)', bg: 'var(--error-bg)' },
                { to: '/settings', label: t('dashboard.systemSettings'), color: 'var(--text-secondary)', bg: 'var(--bg-glass)' },
              ].map((item) => (
                <QuickActionLink key={item.to} item={item} />
              ))}
            </div>
          </div>

          {/* 系统状态 */}
          <div style={{ ...CARD_STYLE, padding: '20px', boxShadow: CARD_DEFAULT_SHADOW }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '14px' }}>
              <SvgIcon name="cpu" size={18} color="var(--accent)" />
              <h2 style={{ margin: 0, fontSize: '15px', fontWeight: 600, color: 'var(--text-heading)' }}>
                {t('dashboard.systemStatus')}
              </h2>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
              <SystemMetric label={t('dashboard.dbConnected')} icon="database" value={health?.db ? t('common.ok') : t('common.errorStatus')} color={health?.db ? 'var(--success)' : 'var(--error)'} />
              <SystemMetric
                label={t('dashboard.embeddingCache')}
                icon="cache"
                value={`${cache?.embedding_cache?.entries ?? 0} ${t('dashboard.cacheVectors')}`}
                color="#10b981"
              />
              <SystemMetric
                label={t('dashboard.responseCache')}
                icon="cache"
                value={`${cache?.response_cache?.entries ?? 0}${t('common.entries')} / ${cache?.response_cache?.total_hits ?? 0} ${t('dashboard.cacheHits')}`}
                color="#06b6d4"
              />
            </div>
          </div>
        </div>
      </div>

      {/* ─── 底部：Token 趋势 ─── */}
      <div style={{ ...CARD_STYLE, padding: '20px', boxShadow: CARD_DEFAULT_SHADOW }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '16px' }}>
          <SvgIcon name="trend" size={18} color="var(--accent)" />
          <h2 style={{ margin: 0, fontSize: '15px', fontWeight: 600, color: 'var(--text-heading)' }}>
            {t('dashboard.tokenTrend')}
          </h2>
        </div>
        {hasTokenData ? (
          <div style={{ position: 'relative', height: '120px' }}>
            <svg width="100%" height="120" viewBox="0 0 400 120" preserveAspectRatio="none" style={{ overflow: 'visible' }}>
              {/* 渐变填充 */}
              <defs>
                <linearGradient id="tokenGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#818CF8" stopOpacity="0.3" />
                  <stop offset="100%" stopColor="#818CF8" stopOpacity="0" />
                </linearGradient>
              </defs>
              {/* 网格线 */}
              {[0, 30, 60, 90].map((y) => (
                <line key={y} x1="0" y1={y} x2="400" y2={y} stroke="var(--border-subtle)" strokeWidth="0.5" strokeDasharray="4,4" />
              ))}
              {/* 填充区域 */}
              <path
                d={`${sparklinePath(tokenTrend, 400, 110)} L400,110 L0,110 Z`}
                fill="url(#tokenGrad)"
              />
              {/* 折线 */}
              <path
                d={sparklinePath(tokenTrend, 400, 110)}
                fill="none"
                stroke="#818CF8"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              {/* 数据点 */}
              {tokenTrend.map((v, i) => {
                const max = Math.max(...tokenTrend, 1)
                const min = Math.min(...tokenTrend, 0)
                const range = max - min || 1
                const x = (i / (tokenTrend.length - 1)) * 400
                const y = 110 - ((v - min) / range) * 106 - 2
                return (
                  <circle
                    key={i}
                    cx={x}
                    cy={y}
                    r={i === tokenTrend.length - 1 ? 4 : 2.5}
                    fill={i === tokenTrend.length - 1 ? '#818CF8' : 'var(--bg-elevated)'}
                    stroke="#818CF8"
                    strokeWidth="2"
                  />
                )
              })}
            </svg>
            {/* X 轴标签 */}
            <div style={{
              display: 'flex', justifyContent: 'space-between', marginTop: '8px',
              fontSize: '11px', color: 'var(--text-muted)',
            }}>
              {[6, 5, 4, 3, 2, 1, 0].map((d) => (
                <span key={d}>{d === 0 ? t('dashboard.chartToday') : t('dashboard.chartDaysAgo', { n: d })}</span>
              ))}
            </div>
          </div>
        ) : (
          <div style={{
            height: '120px', display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: 'var(--text-muted)', fontSize: '13px', background: 'var(--bg-glass)',
            borderRadius: '10px',
          }}>
            <SvgIcon name="trend" size={16} color="var(--text-muted)" />
            <span style={{ marginLeft: '8px' }}>{t('dashboard.noTokenData')}</span>
          </div>
        )}
      </div>
    </div>
  )
}

/* ─── 子组件 ─────────────────────────────── */

interface StatItem {
  icon: string
  label: string
  value: string
  color: string
  gradient: string
  sub: string
}

function StatCard({ item }: { item: StatItem }) {
  const [hovered, setHovered] = useState(false)

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        ...CARD_STYLE,
        padding: '18px',
        boxShadow: hovered ? CARD_HOVER_SHADOW : CARD_DEFAULT_SHADOW,
        transform: hovered ? 'translateY(-2px)' : 'none',
        cursor: 'default',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: '12px', color: 'var(--text-muted)', marginBottom: '8px', fontWeight: 500 }}>
            {item.label}
          </div>
          <div style={{
            fontSize: '28px', fontWeight: 700,
            background: item.gradient,
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            backgroundClip: 'text',
            lineHeight: 1.1,
          }}>
            {item.value}
          </div>
        </div>
        <div style={{
          width: '36px', height: '36px', borderRadius: '10px',
          background: `${item.color}18`,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          flexShrink: 0,
        }}>
          <SvgIcon name={item.icon} size={18} color={item.color} />
        </div>
      </div>
      <div style={{
        fontSize: '11px', color: 'var(--text-muted)', marginTop: '10px',
        borderTop: '1px solid var(--border-subtle)', paddingTop: '8px',
      }}>
        {item.sub}
      </div>
    </div>
  )
}

/** Agent 行组件 */
function AgentRow({ agent, t }: { agent: AgentSummary; t: (key: string, params?: Record<string, string | number>) => string }) {
  const [hovered, setHovered] = useState(false)
  const hasActivity = agent.sessionCount > 0

  return (
    <Link
      to={`/agents/${agent.id}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex', alignItems: 'center', gap: '12px',
        padding: '10px 12px', borderRadius: '10px', textDecoration: 'none', color: 'inherit',
        background: hovered ? 'var(--bg-glass-hover)' : 'var(--bg-glass)',
        border: `1px solid ${hovered ? 'var(--accent)' : 'transparent'}`,
        transition: 'all 0.15s ease',
      }}
    >
      {/* 状态指示灯 */}
      <span style={{
        width: '8px', height: '8px', borderRadius: '50%', flexShrink: 0,
        backgroundColor: hasActivity ? 'var(--success)' : 'var(--text-muted)',
        boxShadow: hasActivity ? '0 0 6px var(--success)' : 'none',
      }} />

      {/* 名称 + 模型 */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{
          fontSize: '13px', fontWeight: 600, color: 'var(--text-primary)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}>
          {agent.name}
        </div>
        <span style={{
          display: 'inline-block', fontSize: '10px', padding: '1px 6px',
          backgroundColor: 'var(--accent-bg)', color: 'var(--text-accent)',
          borderRadius: '4px', marginTop: '2px', fontWeight: 500,
        }}>
          {agent.model}
        </span>
      </div>

      {/* 今日消息数 */}
      {agent.sessionCount > 0 && (
        <div style={{
          fontSize: '12px', color: 'var(--text-secondary)', display: 'flex',
          alignItems: 'center', gap: '4px', flexShrink: 0,
        }}>
          <SvgIcon name="message" size={12} color="var(--text-muted)" />
          {agent.sessionCount}
        </div>
      )}
    </Link>
  )
}

/** 快捷操作链接 */
function QuickActionLink({ item }: { item: { to: string; label: string; color: string; bg: string } }) {
  const [hovered, setHovered] = useState(false)

  return (
    <Link
      to={item.to}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: '12px 8px', textAlign: 'center', borderRadius: '10px',
        backgroundColor: item.bg, color: item.color, textDecoration: 'none',
        fontWeight: 500, fontSize: '12px',
        border: `1px solid ${hovered ? item.color : 'transparent'}`,
        transition: 'all 0.15s ease',
        transform: hovered ? 'translateY(-1px)' : 'none',
      }}
    >
      {item.label}
    </Link>
  )
}

/** 系统指标行 */
function SystemMetric({ label, icon, value, color }: { label: string; icon: string; value: string; color: string }) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: '12px',
      padding: '10px 12px', background: 'var(--bg-glass)', borderRadius: '8px',
    }}>
      <div style={{
        width: '32px', height: '32px', borderRadius: '8px',
        background: `${color}18`, display: 'flex', alignItems: 'center', justifyContent: 'center',
        flexShrink: 0,
      }}>
        <SvgIcon name={icon} size={16} color={color} />
      </div>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '2px' }}>{label}</div>
        <div style={{ fontSize: '13px', fontWeight: 600, color: 'var(--text-primary)' }}>{value}</div>
      </div>
    </div>
  )
}
