/**
 * Agent 列表页
 *
 * 卡片式展示所有 Agent，支持创建、删除、快速进入对话
 */

import { useState, useEffect, useCallback } from 'react'
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

export default function AgentListPage() {
  const navigate = useNavigate()
  const { t } = useI18n()
  const [agents, setAgents] = useState<Agent[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null)

  const loadAgents = useCallback(async () => {
    try {
      setLoading(true)
      const result = await invoke<Agent[]>('list_agents')
      setAgents(result)
      setError('')
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

  const formatTime = (ts: number) => {
    if (!ts) return t('common.unknown')
    const d = new Date(ts)
    const now = new Date()
    const diff = now.getTime() - d.getTime()
    if (diff < 60000) return t('common.justNow')
    if (diff < 3600000) return t('common.minutesAgo', { n: Math.floor(diff / 60000) })
    if (diff < 86400000) return t('common.hoursAgo', { n: Math.floor(diff / 3600000) })
    return d.toLocaleDateString()
  }

  const getModelColor = (model: string) => {
    if (model.includes('gpt-4')) return '#10a37f'
    if (model.includes('gpt-3')) return '#19c37d'
    if (model.includes('claude')) return '#d97706'
    if (model.includes('deepseek')) return '#6366f1'
    if (model.includes('qwen')) return '#0ea5e9'
    return '#6b7280'
  }

  if (loading) {
    return <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('common.loading')}</div>
  }

  return (
    <div style={{ padding: '24px 32px', maxWidth: 1200 }}>
      {/* 头部 */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
        <div>
          <h1 style={{ margin: 0, fontSize: 24, fontWeight: 600 }}>{t('agents.title')}</h1>
          <p style={{ margin: '4px 0 0', color: 'var(--text-secondary)', fontSize: 14 }}>
            {t('agents.subtitle', { count: agents.length })}
          </p>
        </div>
        <button
          onClick={() => navigate('/agents/new')}
          style={{
            padding: '10px 20px',
            backgroundColor: 'var(--accent)',
            color: 'white',
            border: 'none',
            borderRadius: 8,
            fontSize: 14,
            fontWeight: 500,
            cursor: 'pointer',
          }}
        >
          {t('agents.btnCreate')}
        </button>
      </div>

      {error && (
        <div style={{ padding: 12, backgroundColor: 'var(--error-bg)', color: 'var(--error)', borderRadius: 8, marginBottom: 16, fontSize: 14 }}>
          {error}
        </div>
      )}

      {/* Agent 卡片网格 */}
      {agents.length === 0 ? (
        <div style={{
          textAlign: 'center', padding: '60px 20px',
          backgroundColor: 'var(--bg-glass)', borderRadius: 12, border: '2px dashed #e5e7eb',
        }}>
          <div style={{ fontSize: 48, marginBottom: 16 }}>🤖</div>
          <h3 style={{ margin: '0 0 8px', color: 'var(--text-primary)' }}>{t('agents.emptyTitle')}</h3>
          <p style={{ color: 'var(--text-secondary)', marginBottom: 20 }}>{t('agents.emptyDesc')}</p>
          <button
            onClick={() => navigate('/agents/new')}
            style={{
              padding: '10px 24px', backgroundColor: 'var(--accent)', color: 'white',
              border: 'none', borderRadius: 8, fontSize: 14, cursor: 'pointer',
            }}
          >
            {t('dashboard.createAgent')}
          </button>
        </div>
      ) : (
        <div style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))',
          gap: 16,
        }}>
          {agents.map((agent) => (
            <div
              key={agent.id}
              style={{
                backgroundColor: 'white',
                border: '1px solid var(--border-subtle)',
                borderRadius: 12,
                padding: 20,
                cursor: 'pointer',
                transition: 'box-shadow 0.2s, border-color 0.2s',
              }}
              onClick={() => navigate(`/agents/${agent.id}`)}
              onMouseEnter={(e) => {
                e.currentTarget.style.boxShadow = '0 4px 12px rgba(0,0,0,0.08)'
                e.currentTarget.style.borderColor = 'var(--accent)'
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.boxShadow = 'none'
                e.currentTarget.style.borderColor = '#e5e7eb'
              }}
            >
              {/* 卡片头部 */}
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', marginBottom: 12 }}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {agent.name}
                  </h3>
                  <span style={{
                    display: 'inline-block', marginTop: 6,
                    padding: '2px 8px', borderRadius: 4, fontSize: 12,
                    backgroundColor: getModelColor(agent.model) + '15',
                    color: getModelColor(agent.model),
                    fontWeight: 500,
                  }}>
                    {agent.model}
                  </span>
                </div>
                <div style={{
                  width: 40, height: 40, borderRadius: 10,
                  backgroundColor: 'var(--bg-glass)', display: 'flex',
                  alignItems: 'center', justifyContent: 'center', fontSize: 20,
                }}>
                  🤖
                </div>
              </div>

              {/* 描述 */}
              <p style={{
                margin: '0 0 16px', fontSize: 13, color: 'var(--text-secondary)',
                overflow: 'hidden', textOverflow: 'ellipsis',
                display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' as const,
                lineHeight: '1.5',
              }}>
                {agent.systemPrompt || t('agents.noDescription')}
              </p>

              {/* 底部信息 */}
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                  {t('agents.updatedAt')} {formatTime(agent.updatedAt)}
                </span>
                {/* 快速操作 */}
                <div style={{ display: 'flex', gap: 4 }} onClick={(e) => e.stopPropagation()}>
                  <button
                    onClick={() => navigate(`/agents/${agent.id}?tab=chat`)}
                    title={t('agents.actionChat')}
                    style={{
                      padding: '4px 8px', border: '1px solid var(--border-subtle)', borderRadius: 6,
                      backgroundColor: 'white', cursor: 'pointer', fontSize: 14,
                    }}
                  >
                    💬
                  </button>
                  <button
                    onClick={() => navigate(`/agents/${agent.id}?tab=settings`)}
                    title={t('agents.actionSettings')}
                    style={{
                      padding: '4px 8px', border: '1px solid var(--border-subtle)', borderRadius: 6,
                      backgroundColor: 'white', cursor: 'pointer', fontSize: 14,
                    }}
                  >
                    ⚙️
                  </button>
                  <button
                    onClick={() => setDeleteConfirm(agent.id)}
                    title={t('agents.actionDelete')}
                    style={{
                      padding: '4px 8px', border: '1px solid #fecaca', borderRadius: 6,
                      backgroundColor: 'white', cursor: 'pointer', fontSize: 14,
                    }}
                  >
                    🗑️
                  </button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* 删除确认弹窗 */}
      {deleteConfirm && (
        <div style={{
          position: 'fixed', inset: 0, backgroundColor: 'rgba(0,0,0,0.4)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 1000,
        }}>
          <div style={{
            backgroundColor: 'white', borderRadius: 12, padding: 24,
            maxWidth: 400, width: '90%',
          }}>
            <h3 style={{ margin: '0 0 8px' }}>{t('agents.confirmDeleteTitle')}</h3>
            <p style={{ color: 'var(--text-secondary)', margin: '0 0 20px' }}>
              {t('agents.confirmDeleteDesc')}
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button
                onClick={() => setDeleteConfirm(null)}
                style={{
                  padding: '8px 16px', border: '1px solid var(--border-subtle)', borderRadius: 6,
                  backgroundColor: 'white', cursor: 'pointer',
                }}
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={() => handleDelete(deleteConfirm)}
                style={{
                  padding: '8px 16px', border: 'none', borderRadius: 6,
                  backgroundColor: 'var(--error)', color: 'white', cursor: 'pointer',
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
