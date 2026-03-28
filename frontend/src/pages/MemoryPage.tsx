/**
 * 记忆管理页
 *
 * 展示 Agent 记忆体 + 对话/消息统计，支持导出快照、清理、搜索
 * 后端 API: export_memory_snapshot, run_memory_hygiene, get_agent_detail
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { useConfirm } from '../hooks/useConfirm'
import Select from '../components/Select'

interface Memory {
  id: string
  memory_type: string
  content: string
  priority: number
  created_at: number
  updated_at: number
}

interface AgentStats {
  sessionCount: number
  conversationCount: number
  messageCount: number
  vectorCount: number
  embeddingCacheCount: number
}

/** 搜索图标 SVG 组件 */
function SearchIcon() {
  return (
    <svg
      width="15" height="15" viewBox="0 0 24 24" fill="none"
      stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
      style={{ position: 'absolute', left: '12px', top: '50%', transform: 'translateY(-50%)', pointerEvents: 'none' }}
    >
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  )
}

/** 记忆类型图标 */
function MemoryTypeIcon({ type }: { type: string }) {
  const paths: Record<string, string> = {
    core: 'M12 2L2 7l10 5 10-5-10-5z M2 17l10 5 10-5 M2 12l10 5 10-5',
    episodic: 'M12 8v4l3 3 M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z',
    semantic: 'M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z',
    procedural: 'M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z',
  }
  const d = paths[type] || 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M12 8v4 M12 16h.01'
  return (
    <svg
      width="14" height="14" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
    >
      <path d={d} />
    </svg>
  )
}

export default function MemoryPage() {
  const { t } = useI18n()
  const confirm = useConfirm()
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [memories, setMemories] = useState<Memory[]>([])
  const [stats, setStats] = useState<AgentStats>({ sessionCount: 0, conversationCount: 0, messageCount: 0, vectorCount: 0, embeddingCacheCount: 0 })
  const [searchQuery, setSearchQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const [actionLoading, setActionLoading] = useState(false)
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({})

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { if (selectedAgent) loadData() }, [selectedAgent])

  const loadAgents = async () => {
    try {
      const list = (await invoke('list_agents')) as Array<{ id: string; name: string }>
      setAgents(list.map((a) => ({ id: a.id, name: a.name })))
      if (list.length > 0) setSelectedAgent(list[0].id)
    } catch (e) {
      showMsg('error', 'Failed to load agents')
    }
    setLoading(false)
  }

  const loadData = async () => {
    try {
      const detail = (await invoke('get_agent_detail', { agentId: selectedAgent })) as { memories?: Memory[] } & Partial<AgentStats>
      setMemories(detail?.memories || [])
      setStats({
        sessionCount: detail?.sessionCount || 0,
        conversationCount: detail?.conversationCount || 0,
        messageCount: detail?.messageCount || 0,
        vectorCount: detail?.vectorCount || 0,
        embeddingCacheCount: detail?.embeddingCacheCount || 0,
      })
      // 默认展开所有分组
      const types = (detail?.memories || []).reduce<Record<string, boolean>>((acc, m) => {
        acc[m.memory_type || 'uncategorized'] = true
        return acc
      }, {})
      setExpandedGroups(types)
    } catch (e) {
      setMemories([])
      setStats({ sessionCount: 0, conversationCount: 0, messageCount: 0, vectorCount: 0, embeddingCacheCount: 0 })
    }
  }

  const handleExtract = async () => {
    if (!await confirm(t('memory.confirmExtract'))) return
    setActionLoading(true)
    try {
      const result = (await invoke('extract_memories_from_history', { agentId: selectedAgent })) as { message?: string; extracted?: number }
      showMsg('success', result?.message || t('memory.successExtracted', { count: result?.extracted || 0 }))
      await loadData()
    } catch (e) {
      showMsg('error', 'Extract failed: ' + String(e))
    }
    setActionLoading(false)
  }

  const handleExport = async () => {
    setActionLoading(true)
    try {
      const result = (await invoke('export_memory_snapshot', { agentId: selectedAgent })) as string
      showMsg('success', result)
    } catch (e) {
      showMsg('error', 'Export failed: ' + String(e))
    }
    setActionLoading(false)
  }

  const handleHygiene = async () => {
    if (!await confirm(t('memory.confirmHygiene'))) return
    setActionLoading(true)
    try {
      const result = (await invoke('run_memory_hygiene', { agentId: selectedAgent })) as string
      showMsg('success', result)
      await loadData()
    } catch (e) {
      showMsg('error', 'Cleanup failed: ' + String(e))
    }
    setActionLoading(false)
  }

  const showMsg = (type: 'success' | 'error', text: string) => {
    setMessage({ type, text })
    setTimeout(() => setMessage(null), 4000)
  }

  const toggleGroup = (type: string) => {
    setExpandedGroups(prev => ({ ...prev, [type]: !prev[type] }))
  }

  const filteredMemories = memories.filter((m) =>
    m.content.toLowerCase().includes(searchQuery.toLowerCase()) ||
    m.memory_type.toLowerCase().includes(searchQuery.toLowerCase())
  )

  // 按类型分组
  const grouped = filteredMemories.reduce<Record<string, Memory[]>>((acc, m) => {
    const type = m.memory_type || t('common.uncategorized')
    ;(acc[type] = acc[type] || []).push(m)
    return acc
  }, {})

  const typeColors: Record<string, { bg: string; text: string; glow: string }> = {
    core:       { bg: 'rgba(99,102,241,0.12)', text: '#818cf8', glow: 'rgba(99,102,241,0.06)' },
    episodic:   { bg: 'rgba(245,158,11,0.12)', text: '#fbbf24', glow: 'rgba(245,158,11,0.06)' },
    semantic:   { bg: 'rgba(34,197,94,0.12)', text: '#4ade80', glow: 'rgba(34,197,94,0.06)' },
    procedural: { bg: 'rgba(168,85,247,0.12)', text: '#c084fc', glow: 'rgba(168,85,247,0.06)' },
  }

  if (loading) return <div style={{ padding: '24px', color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: '24px', maxWidth: '900px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
        <h1 style={{ margin: 0, fontSize: '22px', fontWeight: 600 }}>{t('memory.title')}</h1>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button
            onClick={handleExtract}
            disabled={actionLoading}
            style={{
              padding: '8px 16px', fontSize: '13px', backgroundColor: '#7C3AED',
              color: 'white', border: 'none', borderRadius: '6px', cursor: 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {actionLoading ? t('common.processing') : t('memory.btnExtract')}
          </button>
          <button
            onClick={handleExport}
            disabled={actionLoading}
            style={{
              padding: '8px 16px', fontSize: '13px', backgroundColor: 'var(--accent)',
              color: 'white', border: 'none', borderRadius: '6px', cursor: 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {t('memory.btnExport')}
          </button>
          <button
            onClick={handleHygiene}
            disabled={actionLoading}
            style={{
              padding: '8px 16px', fontSize: '13px', backgroundColor: 'var(--warning)',
              color: 'white', border: 'none', borderRadius: '6px', cursor: 'pointer',
              opacity: actionLoading ? 0.6 : 1,
            }}
          >
            {t('memory.btnClean')}
          </button>
        </div>
      </div>

      {/* Agent 选择器 + 搜索 */}
      <div style={{ display: 'flex', gap: '12px', marginBottom: '16px' }}>
        <Select
          value={selectedAgent}
          onChange={setSelectedAgent}
          options={agents.map((a) => ({ value: a.id, label: a.name }))}
          style={{ minWidth: 160 }}
        />
        <div style={{ flex: 1, position: 'relative' }}>
          <SearchIcon />
          <input
            type="text"
            placeholder={t('memory.searchPlaceholder')}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            style={{
              width: '100%', padding: '8px 12px 8px 36px', border: '1px solid var(--border-subtle)',
              borderRadius: '8px', fontSize: '13px', boxSizing: 'border-box',
              backgroundColor: 'var(--bg-elevated)', color: 'var(--text-primary)',
              outline: 'none',
            }}
          />
        </div>
      </div>

      {message && (
        <div style={{
          padding: '10px 14px', borderRadius: '6px', marginBottom: '12px', fontSize: '13px',
          backgroundColor: message.type === 'success' ? 'var(--success-bg)' : 'var(--error-bg)',
          color: message.type === 'success' ? 'var(--success)' : 'var(--error)',
        }}>
          {message.text}
        </div>
      )}

      {/* 数据统计卡片 - glass 风格 */}
      <div style={{
        display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
        gap: '12px', marginBottom: '20px',
      }}>
        <MiniCard label={t('memory.statCount')} value={memories.length} color="#8b5cf6" icon="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2 M9 5a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v0a2 2 0 0 1-2 2h-2a2 2 0 0 1-2-2z" />
        <MiniCard label={t('memory.statVectors')} value={stats.vectorCount} color="#f59e0b" icon="M12 2L2 7l10 5 10-5-10-5z M2 17l10 5 10-5 M2 12l10 5 10-5" />
        <MiniCard label={t('memory.statSessions')} value={stats.sessionCount} color="#3b82f6" icon="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
        <MiniCard label={t('memory.statMessages')} value={stats.messageCount} color="#10b981" icon="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z M22 6l-10 7L2 6" />
        <MiniCard label={t('memory.statEmbeddingCache')} value={stats.embeddingCacheCount} color="#06b6d4" icon="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M2 12h20 M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
      </div>

      {/* 记忆列表 - 按分组 */}
      {filteredMemories.length === 0 ? (
        <div style={{
          padding: '48px 24px', textAlign: 'center', borderRadius: '12px',
          background: 'linear-gradient(135deg, rgba(255,255,255,0.03), rgba(255,255,255,0.06))',
          backdropFilter: 'blur(12px)',
          border: '1px solid rgba(255,255,255,0.08)',
        }}>
          {/* 空状态 SVG 图标 */}
          <div style={{ marginBottom: '16px', opacity: 0.4 }}>
            <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z" />
              <path d="M12 8a2.5 2.5 0 0 1 0 5" />
              <path d="M12 16h.01" />
            </svg>
          </div>
          <div style={{ color: 'var(--text-secondary)', fontSize: '15px', marginBottom: '8px', fontWeight: 500 }}>
            {memories.length === 0 ? t('memory.emptyMemories') : t('memory.emptySearch')}
          </div>
          {memories.length === 0 && (
            <div style={{ color: 'var(--text-muted)', fontSize: '13px', lineHeight: '1.6' }}>
              {t('memory.hintAutoAccumulate')}
              {stats.messageCount > 0 && (
                <div style={{ marginTop: '10px', color: 'var(--text-secondary)', fontSize: '12px' }}>
                  {stats.messageCount} {t('memory.statMessages')} &middot; {stats.sessionCount} {t('memory.statSessions')}
                </div>
              )}
            </div>
          )}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
          {Object.entries(grouped).map(([type, items]) => {
            const colors = typeColors[type] || { bg: 'rgba(156,163,175,0.12)', text: '#9ca3af', glow: 'rgba(156,163,175,0.06)' }
            const isExpanded = expandedGroups[type] !== false
            return (
              <div key={type}>
                {/* 分组头 */}
                <button
                  onClick={() => toggleGroup(type)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: '8px', width: '100%',
                    padding: '8px 12px', marginBottom: '8px',
                    backgroundColor: colors.bg, border: 'none', borderRadius: '8px',
                    cursor: 'pointer', color: colors.text, fontSize: '13px', fontWeight: 600,
                  }}
                >
                  <MemoryTypeIcon type={type} />
                  <span>{type}</span>
                  <span style={{ fontSize: '11px', fontWeight: 400, opacity: 0.7 }}>({items.length})</span>
                  <svg
                    width="12" height="12" viewBox="0 0 24 24" fill="none"
                    stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                    style={{
                      marginLeft: 'auto',
                      transition: 'transform 0.2s ease',
                      transform: isExpanded ? 'rotate(180deg)' : 'rotate(0deg)',
                    }}
                  >
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                </button>

                {/* 分组内容 */}
                {isExpanded && (
                  <div style={{ display: 'grid', gap: '8px' }}>
                    {items.map(m => (
                      <MemoryCard key={m.id} memory={m} colors={colors} t={t} />
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

/** 单个记忆卡片 - glass-card 风格 */
function MemoryCard({ memory: m, colors, t }: {
  memory: Memory
  colors: { bg: string; text: string; glow: string }
  t: (key: string, params?: Record<string, string | number>) => string
}) {
  const [hovered, setHovered] = useState(false)

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        padding: '14px 16px',
        background: hovered
          ? 'linear-gradient(135deg, rgba(255,255,255,0.06), rgba(255,255,255,0.1))'
          : 'linear-gradient(135deg, rgba(255,255,255,0.03), rgba(255,255,255,0.06))',
        backdropFilter: 'blur(12px)',
        border: `1px solid ${hovered ? 'rgba(255,255,255,0.12)' : 'rgba(255,255,255,0.06)'}`,
        borderRadius: '10px',
        transition: 'all 0.2s ease',
        transform: hovered ? 'translateY(-1px)' : 'none',
        boxShadow: hovered ? `0 8px 24px ${colors.glow}` : 'none',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
        <span style={{
          fontSize: '11px', padding: '2px 10px', borderRadius: '9999px',
          backgroundColor: colors.bg, color: colors.text, fontWeight: 500,
        }}>
          {m.memory_type}
        </span>
        <span style={{
          fontSize: '10px', padding: '2px 6px', borderRadius: '4px',
          backgroundColor: 'rgba(255,255,255,0.06)', color: 'var(--text-muted)',
        }}>
          P{m.priority}
        </span>
        <span style={{ fontSize: '11px', color: 'var(--text-muted)', marginLeft: 'auto' }}>
          {formatTime(m.updated_at || m.created_at, t)}
        </span>
      </div>
      <div style={{
        fontSize: '13px', color: 'var(--text-primary)', lineHeight: '1.6',
        whiteSpace: 'pre-wrap', wordBreak: 'break-word',
        maxHeight: '120px', overflow: 'auto',
      }}>
        {m.content}
      </div>
    </div>
  )
}

/** 统计迷你卡片 - glass 风格 */
function MiniCard({ label, value, color, icon }: { label: string; value: number; color: string; icon: string }) {
  return (
    <div style={{
      padding: '14px 16px',
      background: 'linear-gradient(135deg, rgba(255,255,255,0.03), rgba(255,255,255,0.06))',
      backdropFilter: 'blur(12px)',
      border: '1px solid rgba(255,255,255,0.06)',
      borderRadius: '10px',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '6px', marginBottom: '4px' }}>
        <svg
          width="13" height="13" viewBox="0 0 24 24" fill="none"
          stroke={color} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ opacity: 0.7 }}
        >
          <path d={icon} />
        </svg>
        <div style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{label}</div>
      </div>
      <div style={{ fontSize: '22px', fontWeight: 700, color }}>{value}</div>
    </div>
  )
}

function formatTime(ts: number, t: (key: string, params?: Record<string, string | number>) => string): string {
  if (!ts) return ''
  const d = new Date(ts)
  const now = Date.now()
  const diff = now - ts
  if (diff < 60_000) return t('common.justNow')
  if (diff < 3600_000) return t('common.minutesAgo', { n: Math.floor(diff / 60_000) })
  if (diff < 86400_000) return t('common.hoursAgo', { n: Math.floor(diff / 3600_000) })
  return d.toLocaleDateString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
}
