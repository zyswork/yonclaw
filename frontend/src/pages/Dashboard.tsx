/**
 * 仪表板 — 系统概览
 *
 * 展示：系统健康状态、Agent 统计、Token 消耗、最近会话、缓存统计
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { Link } from 'react-router-dom'
import { useI18n } from '../i18n'

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

export default function Dashboard() {
  const { t } = useI18n()
  const [health, setHealth] = useState<HealthData | null>(null)
  const [cache, setCache] = useState<CacheStats | null>(null)
  const [agents, setAgents] = useState<AgentSummary[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')

  useEffect(() => {
    loadAll()
  }, [])

  const loadAll = async () => {
    setLoading(true)
    setError('')
    try {
      const [h, c, a] = await Promise.all([
        invoke('health_check').catch(() => null),
        invoke('get_cache_stats').catch(() => null),
        invoke('list_agents').catch(() => []),
      ])
      setHealth(h as HealthData)
      setCache(c as CacheStats)

      // 从 list_agents 提取摘要
      const agentList = (a as any[]) || []
      const summaries: AgentSummary[] = agentList.map((ag: any) => ({
        id: ag.id,
        name: ag.name,
        model: ag.model,
        sessionCount: ag.sessionCount || 0,
      }))
      setAgents(summaries)
    } catch (e) {
      setError(t('dashboard.errorLoading', { error: String(e) }))
    }
    setLoading(false)
  }

  if (loading) {
    return (
      <div style={{ padding: '24px', display: 'flex', justifyContent: 'center', alignItems: 'center', minHeight: '300px' }}>
        <div style={{ textAlign: 'center', color: 'var(--text-muted)' }}>
          <div style={{ fontSize: '24px', marginBottom: '8px' }}>...</div>
          <div>{t('common.loading')}</div>
        </div>
      </div>
    )
  }

  const statusColor = health?.status === 'healthy' ? '#22c55e' : '#ef4444'
  const statusLabel = health?.status === 'healthy' ? t('dashboard.statusHealthy') : t('dashboard.statusUnhealthy')

  return (
    <div style={{ padding: '24px', maxWidth: '1200px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '24px' }}>
        <h1 style={{ margin: 0, fontSize: '22px', fontWeight: 600 }}>{t('dashboard.title')}</h1>
        <button
          onClick={loadAll}
          style={{
            padding: '6px 16px', fontSize: '13px', backgroundColor: 'var(--bg-glass)',
            border: '1px solid var(--border-subtle)', borderRadius: '6px', cursor: 'pointer',
          }}
        >
          {t('dashboard.refresh')}
        </button>
      </div>

      {error && <div style={{ color: 'var(--error)', marginBottom: '16px', padding: '10px', backgroundColor: 'var(--error-bg)', borderRadius: '6px' }}>{error}</div>}

      {/* 状态卡片 */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: '16px', marginBottom: '24px' }}>
        <StatCard
          label={t('dashboard.systemStatus')}
          value={statusLabel}
          valueColor={statusColor}
          icon="pulse"
          sub={health?.db ? t('dashboard.dbConnected') : t('dashboard.dbError')}
        />
        <StatCard
          label={t('dashboard.agentCount')}
          value={String(health?.agents ?? 0)}
          valueColor="#3b82f6"
          icon="agent"
          sub={`${agents.length} ${t('dashboard.registered')}`}
        />
        <StatCard
          label={t('dashboard.todayTokens')}
          value={formatNumber(health?.today_tokens ?? 0)}
          valueColor="#8b5cf6"
          icon="token"
          sub={`${t('common.approxCost')}${((health?.today_tokens ?? 0) / 1000 * 0.003).toFixed(4)}`}
        />
        <StatCard
          label={t('dashboard.memoryCount')}
          value={String(health?.memories ?? 0)}
          valueColor="#f59e0b"
          icon="memory"
          sub={t('dashboard.totalMemory')}
        />
        <StatCard
          label={t('dashboard.responseCache')}
          value={String(cache?.response_cache?.entries ?? 0)}
          valueColor="#06b6d4"
          icon="cache"
          sub={`${t('dashboard.cacheHits')} ${cache?.response_cache?.total_hits ?? 0} ${t('common.times')}`}
        />
        <StatCard
          label={t('dashboard.embeddingCache')}
          value={String(cache?.embedding_cache?.entries ?? 0)}
          valueColor="#10b981"
          icon="embed"
          sub={t('dashboard.cacheVectors')}
        />
      </div>

      {/* Agent 列表 */}
      <div style={{ marginBottom: '24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '12px' }}>
          <h2 style={{ margin: 0, fontSize: '16px', fontWeight: 600 }}>{t('dashboard.agentsOverview')}</h2>
          <Link to="/agents" style={{ fontSize: '13px', color: 'var(--text-accent)', textDecoration: 'none' }}>{t('common.viewAll')} &rarr;</Link>
        </div>
        {agents.length === 0 ? (
          <div style={{ padding: '24px', textAlign: 'center', color: 'var(--text-muted)', backgroundColor: 'var(--bg-glass)', borderRadius: '8px' }}>
            {t('dashboard.emptyAgents')}<Link to="/agents/new" style={{ color: 'var(--text-accent)' }}>{t('dashboard.createOne')}</Link>
          </div>
        ) : (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: '12px' }}>
            {agents.map((agent) => (
              <Link
                key={agent.id}
                to={`/agents/${agent.id}`}
                style={{
                  display: 'block', padding: '14px 16px', backgroundColor: 'var(--bg-glass)',
                  border: '1px solid var(--border-subtle)', borderRadius: '8px', textDecoration: 'none', color: 'inherit',
                  transition: 'border-color 0.2s, box-shadow 0.2s',
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.borderColor = '#3b82f6'
                  e.currentTarget.style.boxShadow = '0 2px 8px rgba(59,130,246,0.1)'
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.borderColor = '#e5e7eb'
                  e.currentTarget.style.boxShadow = 'none'
                }}
              >
                <div style={{ fontWeight: 600, fontSize: '14px', marginBottom: '4px' }}>{agent.name}</div>
                <div style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                  <span style={{
                    display: 'inline-block', padding: '1px 6px', backgroundColor: 'var(--accent-bg)',
                    borderRadius: '4px', marginRight: '8px', color: 'var(--accent)',
                  }}>
                    {agent.model}
                  </span>
                  {agent.sessionCount > 0 && `${agent.sessionCount}${t('common.sessions')}`}
                </div>
              </Link>
            ))}
          </div>
        )}
      </div>

      {/* 快捷操作 */}
      <div>
        <h2 style={{ margin: '0 0 12px', fontSize: '16px', fontWeight: 600 }}>{t('dashboard.quickActions')}</h2>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(160px, 1fr))', gap: '10px' }}>
          {[
            { to: '/agents/new', label: t('dashboard.createAgent'), color: 'var(--text-accent)', bg: '#eff6ff' },
            { to: '/skills', label: t('dashboard.skillManagement'), color: '#8b5cf6', bg: '#f5f3ff' },
            { to: '/memory', label: t('dashboard.memoryManagement'), color: '#f59e0b', bg: '#fffbeb' },
            { to: '/cron', label: t('dashboard.cronJobs'), color: '#06b6d4', bg: '#ecfeff' },
            { to: '/token-monitoring', label: t('dashboard.tokenMonitoring'), color: 'var(--error)', bg: '#fef2f2' },
            { to: '/settings', label: t('dashboard.systemSettings'), color: 'var(--text-secondary)', bg: '#f9fafb' },
          ].map((item) => (
            <Link
              key={item.to}
              to={item.to}
              style={{
                display: 'block', padding: '14px', textAlign: 'center', borderRadius: '8px',
                backgroundColor: item.bg, color: item.color, textDecoration: 'none',
                fontWeight: 500, fontSize: '13px', border: `1px solid transparent`,
                transition: 'border-color 0.2s',
              }}
              onMouseEnter={(e) => { e.currentTarget.style.borderColor = item.color }}
              onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'transparent' }}
            >
              {item.label}
            </Link>
          ))}
        </div>
      </div>
    </div>
  )
}

function StatCard({ label, value, valueColor, icon, sub }: {
  label: string; value: string; valueColor: string; icon: string; sub: string
}) {
  const iconMap: Record<string, string> = {
    pulse: '\u{1F49A}', agent: '\u{1F916}', token: '\u{1F4B0}',
    memory: '\u{1F9E0}', cache: '\u{1F4BE}', embed: '\u{1F50D}',
  }
  return (
    <div style={{
      padding: '16px', backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
      borderRadius: '10px', boxShadow: '0 1px 3px rgba(0,0,0,0.3)',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginBottom: '4px' }}>{label}</div>
          <div style={{ fontSize: '24px', fontWeight: 700, color: valueColor }}>{value}</div>
        </div>
        <div style={{ fontSize: '20px', opacity: 0.6 }}>{iconMap[icon] || ''}</div>
      </div>
      <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '6px' }}>{sub}</div>
    </div>
  )
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}
