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

  const typeColors: Record<string, { bg: string; text: string }> = {
    core: { bg: '#dbeafe', text: '#1e40af' },
    episodic: { bg: '#fef3c7', text: '#92400e' },
    semantic: { bg: '#d1fae5', text: '#065f46' },
    procedural: { bg: '#ede9fe', text: '#5b21b6' },
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
              padding: '8px 16px', fontSize: '13px', backgroundColor: '#D97706',
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
        <select
          value={selectedAgent}
          onChange={(e) => setSelectedAgent(e.target.value)}
          style={{ padding: '8px 12px', borderRadius: '6px', border: '1px solid var(--border-subtle)', fontSize: '13px' }}
        >
          {agents.map((a) => (
            <option key={a.id} value={a.id}>{a.name}</option>
          ))}
        </select>
        <input
          type="text"
          placeholder={t('memory.searchPlaceholder')}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          style={{ flex: 1, padding: '8px 12px', border: '1px solid var(--border-subtle)', borderRadius: '6px', fontSize: '13px' }}
        />
      </div>

      {message && (
        <div style={{
          padding: '10px 14px', borderRadius: '6px', marginBottom: '12px', fontSize: '13px',
          backgroundColor: message.type === 'success' ? '#f0fdf4' : 'var(--error-bg)',
          color: message.type === 'success' ? '#22c55e' : '#ef4444',
        }}>
          {message.text}
        </div>
      )}

      {/* 数据统计卡片 */}
      <div style={{
        display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
        gap: '12px', marginBottom: '20px',
      }}>
        <MiniCard label={t('memory.statCount')} value={memories.length} color="#8b5cf6" />
        <MiniCard label={t('memory.statVectors')} value={stats.vectorCount} color="#f59e0b" />
        <MiniCard label={t('memory.statSessions')} value={stats.sessionCount} color="#3b82f6" />
        <MiniCard label={t('memory.statMessages')} value={stats.messageCount} color="#10b981" />
        <MiniCard label={t('memory.statEmbeddingCache')} value={stats.embeddingCacheCount} color="#06b6d4" />
      </div>

      {/* 记忆类型分布 */}
      {Object.keys(grouped).length > 0 && (
        <div style={{
          display: 'flex', gap: '12px', marginBottom: '16px', flexWrap: 'wrap',
        }}>
          {Object.keys(grouped).map((type) => {
            const colors = typeColors[type] || { bg: '#f3f4f6', text: '#374151' }
            return (
              <span key={type} style={{
                fontSize: '12px', padding: '3px 10px', borderRadius: '6px',
                backgroundColor: colors.bg, color: colors.text, fontWeight: 500,
              }}>
                {type}: {grouped[type].length}
              </span>
            )
          })}
        </div>
      )}

      {/* 记忆列表 */}
      {filteredMemories.length === 0 ? (
        <div style={{
          padding: '40px', textAlign: 'center', borderRadius: '8px',
          backgroundColor: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
        }}>
          <div style={{ fontSize: '32px', marginBottom: '12px', opacity: 0.4 }}>{'\u{1F9E0}'}</div>
          <div style={{ color: 'var(--text-secondary)', fontSize: '14px', marginBottom: '8px' }}>
            {memories.length === 0 ? t('memory.emptyMemories') : t('memory.emptySearch')}
          </div>
          {memories.length === 0 && (
            <div style={{ color: 'var(--text-muted)', fontSize: '12px', lineHeight: '1.6' }}>
              {t('memory.hintAutoAccumulate')}
              {stats.messageCount > 0 && (
                <div style={{ marginTop: '8px', color: 'var(--text-secondary)' }}>
                  {stats.messageCount} {t('memory.statMessages')} · {stats.sessionCount} {t('memory.statSessions')}
                </div>
              )}
            </div>
          )}
        </div>
      ) : (
        <div style={{ display: 'grid', gap: '8px' }}>
          {filteredMemories.map((m) => {
            const colors = typeColors[m.memory_type] || { bg: '#f3f4f6', text: '#374151' }
            return (
              <div
                key={m.id}
                style={{
                  padding: '12px 16px', backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
                  borderRadius: '8px',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '6px' }}>
                  <span style={{
                    fontSize: '11px', padding: '1px 8px', borderRadius: '4px',
                    backgroundColor: colors.bg, color: colors.text, fontWeight: 500,
                  }}>
                    {m.memory_type}
                  </span>
                  <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                    P{m.priority}
                  </span>
                  <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>|</span>
                  <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>
                    {formatTime(m.updated_at || m.created_at, t)}
                  </span>
                </div>
                <div style={{
                  fontSize: '13px', color: 'var(--text-primary)', lineHeight: '1.5',
                  whiteSpace: 'pre-wrap', wordBreak: 'break-word',
                  maxHeight: '120px', overflow: 'auto',
                }}>
                  {m.content}
                </div>
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

function MiniCard({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <div style={{
      padding: '12px 14px', backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
      borderRadius: '8px',
    }}>
      <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '2px' }}>{label}</div>
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
