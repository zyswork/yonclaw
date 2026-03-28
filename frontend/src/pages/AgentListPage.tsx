/**
 * Agent 列表页
 *
 * 深色毛玻璃卡片网格，展示所有 Agent
 * 支持搜索、创建、删除、快速进入对话
 */

import { useState, useEffect, useCallback, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../i18n'

interface Agent {
  id: string
  name: string
  model: string
  systemPrompt: string
  temperature: number | null
  maxTokens: number | null
  configVersion: number | null
  createdAt: number
  updatedAt: number
}

/** 每个 Agent 的统计数据 */
interface AgentStats {
  sessionCount: number
  hasActiveChannel: boolean
}

/** 头像渐变色调色板（10 种） */
const AVATAR_GRADIENTS = [
  'linear-gradient(135deg, #667eea, #764ba2)',
  'linear-gradient(135deg, #f093fb, #f5576c)',
  'linear-gradient(135deg, #4facfe, #00f2fe)',
  'linear-gradient(135deg, #43e97b, #38f9d7)',
  'linear-gradient(135deg, #fa709a, #fee140)',
  'linear-gradient(135deg, #a18cd1, #fbc2eb)',
  'linear-gradient(135deg, #fccb90, #d57eeb)',
  'linear-gradient(135deg, #e0c3fc, #8ec5fc)',
  'linear-gradient(135deg, #f5576c, #ff6a00)',
  'linear-gradient(135deg, #13547a, #80d0c7)',
]

/** 根据名字 hash 选取渐变色 */
function getAvatarGradient(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash)
  }
  return AVATAR_GRADIENTS[Math.abs(hash) % AVATAR_GRADIENTS.length]
}

/** 取名字首字母（中文取第一个字，英文取前两字母大写） */
function getInitials(name: string): string {
  if (!name) return '?'
  // 中文字符
  if (/[\u4e00-\u9fff]/.test(name[0])) return name[0]
  // 英文：取前两个单词首字母
  const parts = name.trim().split(/\s+/)
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase()
  return name.slice(0, 2).toUpperCase()
}

function getModelColor(model: string): string {
  if (model.includes('gpt')) return '#10a37f'
  if (model.includes('claude')) return '#d97706'
  if (model.includes('deepseek')) return '#6366f1'
  if (model.includes('qwen')) return '#0ea5e9'
  if (model.includes('gemini')) return '#ea4335'
  return '#8b8b9a'
}

/** 模型名缩写显示 */
function getModelLabel(model: string): string {
  // 如果太长就截短
  if (model.length > 20) return model.slice(0, 18) + '...'
  return model
}

export default function AgentListPage() {
  const navigate = useNavigate()
  const { t } = useI18n()
  const [agents, setAgents] = useState<Agent[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [stats, setStats] = useState<Record<string, AgentStats>>({})
  const [hoveredCard, setHoveredCard] = useState<string | null>(null)

  const loadAgents = useCallback(async () => {
    try {
      setLoading(true)
      const result = await invoke<Agent[]>('list_agents')
      setAgents(result)
      setError('')

      // 并发加载每个 Agent 的会话数和频道状态
      const statsMap: Record<string, AgentStats> = {}
      await Promise.all(
        result.map(async (agent) => {
          try {
            const sessions = await invoke<{ id: string }[]>('list_sessions', { agentId: agent.id })
            const channels = await invoke<{ enabled: boolean }[]>('list_agent_channels', { agentId: agent.id })
            statsMap[agent.id] = {
              sessionCount: sessions.length,
              hasActiveChannel: channels.some((ch) => ch.enabled),
            }
          } catch {
            statsMap[agent.id] = { sessionCount: 0, hasActiveChannel: false }
          }
        })
      )
      setStats(statsMap)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    loadAgents()
  }, [loadAgents])

  const handleDelete = async (agentId: string) => {
    try {
      await invoke('delete_agent', { agentId })
      setDeleteConfirm(null)
      loadAgents()
    } catch (e) {
      setError(String(e))
    }
  }

  const formatDate = (ts: number) => {
    if (!ts) return t('common.unknown')
    return new Date(ts).toLocaleDateString()
  }

  /** 搜索过滤 */
  const filtered = useMemo(() => {
    if (!search.trim()) return agents
    const q = search.toLowerCase()
    return agents.filter(
      (a) =>
        a.name.toLowerCase().includes(q) ||
        a.model.toLowerCase().includes(q) ||
        (a.systemPrompt && a.systemPrompt.toLowerCase().includes(q))
    )
  }, [agents, search])

  // ─── 加载状态 ───
  if (loading) {
    return (
      <div style={{ padding: '60px 32px', textAlign: 'center' }}>
        <div style={{
          display: 'inline-flex', flexDirection: 'column', alignItems: 'center', gap: 16,
        }}>
          {/* 骨架卡片动画 */}
          <div style={{
            display: 'grid', gridTemplateColumns: 'repeat(3, 200px)', gap: 16,
          }}>
            {[0, 1, 2].map((i) => (
              <div key={i} style={{
                height: 120, borderRadius: 16,
                background: 'var(--bg-glass)',
                border: '1px solid var(--border-subtle)',
                animation: `shimmer 1.5s infinite ${i * 0.2}s`,
              }} />
            ))}
          </div>
          <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>{t('common.loading')}</span>
        </div>
      </div>
    )
  }

  return (
    <div style={{ padding: '28px 32px', maxWidth: 1400 }}>
      {/* ═══ 顶部栏 ═══ */}
      <div style={{
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        marginBottom: 28, flexWrap: 'wrap', gap: 16,
      }}>
        <div>
          <h1 style={{
            margin: 0, fontSize: 26, fontWeight: 700,
            background: 'var(--accent-gradient)',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            backgroundClip: 'text',
          }}>
            {t('agents.title')}
          </h1>
          <p style={{ margin: '4px 0 0', color: 'var(--text-secondary)', fontSize: 13 }}>
            {t('agents.subtitle', { count: agents.length })}
          </p>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          {/* 搜索框 */}
          <div style={{ position: 'relative' }}>
            <span style={{
              position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)',
              color: 'var(--text-muted)', fontSize: 14, pointerEvents: 'none',
            }}>
              &#x1F50D;
            </span>
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t('agentDetail.searchPlaceholder') || 'Search...'}
              style={{
                padding: '9px 14px 9px 36px',
                width: 220,
                borderRadius: 10,
                border: '1px solid var(--border-subtle)',
                backgroundColor: 'var(--bg-glass)',
                color: 'var(--text-primary)',
                fontSize: 13,
                backdropFilter: 'blur(12px)',
                WebkitBackdropFilter: 'blur(12px)',
              }}
            />
          </div>

          {/* 新建按钮 */}
          <button
            onClick={() => navigate('/agents/new')}
            style={{
              padding: '10px 22px',
              background: 'var(--accent-gradient)',
              color: 'white',
              border: 'none',
              borderRadius: 10,
              fontSize: 14,
              fontWeight: 600,
              cursor: 'pointer',
              boxShadow: '0 4px 16px rgba(16, 185, 129, 0.25)',
              transition: 'transform 0.15s, box-shadow 0.15s',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.transform = 'translateY(-1px)'
              e.currentTarget.style.boxShadow = '0 6px 24px rgba(16, 185, 129, 0.35)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.transform = 'none'
              e.currentTarget.style.boxShadow = '0 4px 16px rgba(16, 185, 129, 0.25)'
            }}
          >
            + {t('agents.btnCreate')}
          </button>
        </div>
      </div>

      {/* 错误提示 */}
      {error && (
        <div style={{
          padding: 12, backgroundColor: 'var(--error-bg)', color: 'var(--error)',
          borderRadius: 10, marginBottom: 16, fontSize: 13,
          border: '1px solid rgba(239, 68, 68, 0.2)',
        }}>
          {error}
        </div>
      )}

      {/* ═══ Agent 卡片网格 / 空状态 ═══ */}
      {agents.length === 0 ? (
        /* 空状态 */
        <div style={{
          textAlign: 'center', padding: '80px 20px',
          borderRadius: 20,
          background: 'var(--bg-glass)',
          border: '2px dashed var(--border-subtle)',
          backdropFilter: 'blur(12px)',
          WebkitBackdropFilter: 'blur(12px)',
        }}>
          <div style={{
            width: 80, height: 80, borderRadius: 20, margin: '0 auto 20px',
            background: 'var(--accent-gradient)', display: 'flex',
            alignItems: 'center', justifyContent: 'center', fontSize: 36,
            boxShadow: '0 8px 32px rgba(16, 185, 129, 0.2)',
          }}>
            <span style={{ filter: 'grayscale(0)' }}>&#x2795;</span>
          </div>
          <h3 style={{ margin: '0 0 8px', color: 'var(--text-primary)', fontSize: 18, fontWeight: 600 }}>
            {t('agents.emptyTitle')}
          </h3>
          <p style={{ color: 'var(--text-secondary)', marginBottom: 24, fontSize: 14 }}>
            {t('agents.emptyDesc')}
          </p>
          <button
            onClick={() => navigate('/agents/new')}
            style={{
              padding: '12px 28px',
              background: 'var(--accent-gradient)',
              color: 'white',
              border: 'none',
              borderRadius: 10,
              fontSize: 15,
              fontWeight: 600,
              cursor: 'pointer',
              boxShadow: '0 4px 16px rgba(16, 185, 129, 0.25)',
            }}
          >
            {t('dashboard.createAgent')}
          </button>
        </div>
      ) : filtered.length === 0 ? (
        /* 搜索无结果 */
        <div style={{
          textAlign: 'center', padding: '60px 20px', color: 'var(--text-muted)',
        }}>
          <div style={{ fontSize: 40, marginBottom: 12 }}>&#x1F50D;</div>
          <p style={{ fontSize: 14 }}>No agents found for &quot;{search}&quot;</p>
        </div>
      ) : (
        /* 卡片网格 */
        <div style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
          gap: 18,
        }}>
          {filtered.map((agent) => {
            const isHovered = hoveredCard === agent.id
            const agentStats = stats[agent.id]
            return (
              <div
                key={agent.id}
                onClick={() => navigate(`/agents/${agent.id}`)}
                onMouseEnter={() => setHoveredCard(agent.id)}
                onMouseLeave={() => setHoveredCard(null)}
                style={{
                  position: 'relative',
                  background: 'var(--bg-glass)',
                  backdropFilter: 'blur(12px)',
                  WebkitBackdropFilter: 'blur(12px)',
                  border: isHovered
                    ? '1px solid rgba(16, 185, 129, 0.4)'
                    : '1px solid rgba(255, 255, 255, 0.06)',
                  borderRadius: 16,
                  padding: 22,
                  cursor: 'pointer',
                  transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                  transform: isHovered ? 'translateY(-4px)' : 'none',
                  boxShadow: isHovered
                    ? '0 12px 40px rgba(0, 0, 0, 0.3), 0 0 0 1px rgba(16, 185, 129, 0.15)'
                    : '0 2px 8px rgba(0, 0, 0, 0.15)',
                  overflow: 'hidden',
                }}
              >
                {/* 顶部发光条（hover 时可见） */}
                <div style={{
                  position: 'absolute', top: 0, left: 0, right: 0, height: 2,
                  background: 'var(--accent-gradient)',
                  opacity: isHovered ? 1 : 0,
                  transition: 'opacity 0.25s',
                }} />

                {/* ─── 卡片头部：头像 + 名称 + 模型 ─── */}
                <div style={{ display: 'flex', alignItems: 'flex-start', gap: 14, marginBottom: 14 }}>
                  {/* 头像 */}
                  <div style={{
                    width: 48, height: 48, borderRadius: 14, flexShrink: 0,
                    background: getAvatarGradient(agent.name),
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    fontSize: 18, fontWeight: 700, color: 'white',
                    boxShadow: '0 4px 12px rgba(0, 0, 0, 0.2)',
                    letterSpacing: '-0.02em',
                  }}>
                    {getInitials(agent.name)}
                  </div>

                  <div style={{ flex: 1, minWidth: 0 }}>
                    {/* 名称 */}
                    <h3 style={{
                      margin: 0, fontSize: 16, fontWeight: 600,
                      overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                      color: 'var(--text-primary)',
                    }}>
                      {agent.name}
                    </h3>
                    {/* 模型 badge */}
                    <span style={{
                      display: 'inline-block', marginTop: 5,
                      padding: '2px 8px', borderRadius: 6, fontSize: 11,
                      backgroundColor: getModelColor(agent.model) + '18',
                      color: getModelColor(agent.model),
                      fontWeight: 600, letterSpacing: '0.01em',
                    }}>
                      {getModelLabel(agent.model)}
                    </span>
                  </div>

                  {/* 频道状态指示灯 */}
                  {agentStats?.hasActiveChannel && (
                    <div title="Active channel" style={{
                      width: 8, height: 8, borderRadius: '50%',
                      backgroundColor: 'var(--success)',
                      boxShadow: '0 0 6px rgba(34, 197, 94, 0.6)',
                      marginTop: 6, flexShrink: 0,
                    }} />
                  )}
                </div>

                {/* ─── 中间：描述 ─── */}
                <p style={{
                  margin: '0 0 16px', fontSize: 13, color: 'var(--text-secondary)',
                  overflow: 'hidden', textOverflow: 'ellipsis',
                  display: '-webkit-box',
                  WebkitLineClamp: 2,
                  WebkitBoxOrient: 'vertical' as const,
                  lineHeight: '1.5',
                  minHeight: 39,
                }}>
                  {agent.systemPrompt || t('agents.noDescription')}
                </p>

                {/* ─── 底部：统计 + 操作 ─── */}
                <div style={{
                  display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                  paddingTop: 14,
                  borderTop: '1px solid var(--border-subtle)',
                }}>
                  {/* 统计 */}
                  <div style={{
                    display: 'flex', gap: 12, fontSize: 12, color: 'var(--text-muted)',
                  }}>
                    <span title="Sessions">
                      &#x1F4AC; {agentStats?.sessionCount ?? '...'}
                    </span>
                    <span title="Created">
                      &#x1F4C5; {formatDate(agent.createdAt)}
                    </span>
                  </div>

                  {/* 快速操作 */}
                  <div
                    style={{
                      display: 'flex', gap: 4,
                      opacity: isHovered ? 1 : 0,
                      transition: 'opacity 0.2s',
                    }}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <button
                      onClick={() => navigate(`/agents/${agent.id}?tab=chat`)}
                      title={t('agents.actionChat')}
                      style={{
                        padding: '5px 8px', border: '1px solid var(--border-subtle)',
                        borderRadius: 8, backgroundColor: 'var(--bg-glass)',
                        cursor: 'pointer', fontSize: 13, lineHeight: 1,
                        transition: 'background 0.15s, border-color 0.15s',
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--bg-glass-hover)'
                        e.currentTarget.style.borderColor = 'var(--border-default)'
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--bg-glass)'
                        e.currentTarget.style.borderColor = 'var(--border-subtle)'
                      }}
                    >
                      &#x1F4AC;
                    </button>
                    <button
                      onClick={() => navigate(`/agents/${agent.id}?tab=settings`)}
                      title={t('agents.actionSettings')}
                      style={{
                        padding: '5px 8px', border: '1px solid var(--border-subtle)',
                        borderRadius: 8, backgroundColor: 'var(--bg-glass)',
                        cursor: 'pointer', fontSize: 13, lineHeight: 1,
                        transition: 'background 0.15s, border-color 0.15s',
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--bg-glass-hover)'
                        e.currentTarget.style.borderColor = 'var(--border-default)'
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--bg-glass)'
                        e.currentTarget.style.borderColor = 'var(--border-subtle)'
                      }}
                    >
                      &#x2699;
                    </button>
                    <button
                      onClick={() => setDeleteConfirm(agent.id)}
                      title={t('agents.actionDelete')}
                      style={{
                        padding: '5px 8px', border: '1px solid rgba(239, 68, 68, 0.2)',
                        borderRadius: 8, backgroundColor: 'var(--bg-glass)',
                        cursor: 'pointer', fontSize: 13, lineHeight: 1,
                        transition: 'background 0.15s, border-color 0.15s',
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--error-bg)'
                        e.currentTarget.style.borderColor = 'rgba(239, 68, 68, 0.4)'
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.backgroundColor = 'var(--bg-glass)'
                        e.currentTarget.style.borderColor = 'rgba(239, 68, 68, 0.2)'
                      }}
                    >
                      &#x1F5D1;
                    </button>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* ═══ 删除确认弹窗 ═══ */}
      {deleteConfirm && (
        <div
          style={{
            position: 'fixed', inset: 0,
            backgroundColor: 'rgba(0, 0, 0, 0.5)',
            backdropFilter: 'blur(4px)',
            WebkitBackdropFilter: 'blur(4px)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            zIndex: 1000,
          }}
          onClick={() => setDeleteConfirm(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: 'var(--bg-elevated)',
              backdropFilter: 'blur(20px)',
              WebkitBackdropFilter: 'blur(20px)',
              border: '1px solid var(--border-subtle)',
              borderRadius: 16, padding: 28,
              maxWidth: 420, width: '90%',
              boxShadow: '0 24px 64px rgba(0, 0, 0, 0.4)',
            }}
          >
            <h3 style={{ margin: '0 0 8px', fontSize: 17, fontWeight: 600 }}>
              {t('agents.confirmDeleteTitle')}
            </h3>
            <p style={{ color: 'var(--text-secondary)', margin: '0 0 24px', fontSize: 14, lineHeight: 1.6 }}>
              {t('agents.confirmDeleteDesc')}
            </p>
            <div style={{ display: 'flex', gap: 10, justifyContent: 'flex-end' }}>
              <button
                onClick={() => setDeleteConfirm(null)}
                style={{
                  padding: '9px 18px', border: '1px solid var(--border-subtle)',
                  borderRadius: 8, backgroundColor: 'var(--bg-glass)', cursor: 'pointer',
                  fontSize: 14,
                }}
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={() => handleDelete(deleteConfirm)}
                style={{
                  padding: '9px 18px', border: 'none', borderRadius: 8,
                  backgroundColor: 'var(--error)', color: 'white', cursor: 'pointer',
                  fontSize: 14, fontWeight: 600,
                }}
              >
                {t('agents.btnConfirmDelete')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
