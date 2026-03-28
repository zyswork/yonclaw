/**
 * 审计日志页
 *
 * 展示工具调用审计记录 + 缓存统计 + 系统信息
 * 后端 API: get_audit_log, get_cache_stats, health_check
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import Select from '../components/Select'

interface AuditEntry {
  id: string
  agentId: string
  sessionId: string
  toolName: string
  arguments: string
  result: string
  success: boolean
  policyDecision: string
  policySource: string
  durationMs: number
  createdAt: number
}

export default function AuditLogPage() {
  const { t } = useI18n()
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [entries, setEntries] = useState<AuditEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [page, setPage] = useState(0)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const PAGE_SIZE = 30

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { if (selectedAgent) loadLogs() }, [selectedAgent, page])

  const loadAgents = async () => {
    try {
      const list = (await invoke('list_agents')) as Array<{ id: string; name: string }>
      setAgents(list.map((a) => ({ id: a.id, name: a.name })))
      if (list.length > 0) setSelectedAgent(list[0].id)
    } catch (e) {
      console.error('加载 Agent 失败:', e)
    }
    setLoading(false)
  }

  const loadLogs = async () => {
    try {
      const list = (await invoke('get_audit_log', {
        agentId: selectedAgent,
        limit: PAGE_SIZE,
        offset: page * PAGE_SIZE,
      })) as AuditEntry[]
      setEntries(list)
    } catch (e) {
      console.error('加载审计日志失败:', e)
      setEntries([])
    }
  }

  if (loading) return <div style={{ padding: '24px', color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: '24px', maxWidth: '1100px' }}>
      <h1 style={{ margin: '0 0 20px', fontSize: '22px', fontWeight: 600 }}>{t('audit.title')}</h1>

      {/* 筛选栏 */}
      <div style={{ display: 'flex', gap: '12px', marginBottom: '16px', alignItems: 'center' }}>
        <Select
          value={selectedAgent}
          onChange={(v) => { setSelectedAgent(v); setPage(0) }}
          options={agents.map((a) => ({ value: a.id, label: a.name }))}
          style={{ minWidth: 160 }}
        />
        <div style={{ flex: 1 }} />
        <span style={{ fontSize: '12px', color: 'var(--text-muted)' }}>
          {t('audit.labelTotal')} {entries.length}{t('audit.labelEntries')}{entries.length === PAGE_SIZE ? '+' : ''}
        </span>
      </div>

      {/* 日志列表 */}
      {entries.length === 0 ? (
        <div style={{
          padding: '40px', textAlign: 'center', color: 'var(--text-muted)',
          backgroundColor: 'var(--bg-glass)', borderRadius: '8px', border: '1px solid var(--border-subtle)',
        }}>
          {t('audit.emptyLogs')}
        </div>
      ) : (
        <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '8px', overflow: 'hidden' }}>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13px' }}>
            <thead>
              <tr style={{ backgroundColor: 'var(--bg-glass)' }}>
                <th style={thStyle}>{t('audit.columnTime')}</th>
                <th style={thStyle}>{t('audit.columnTool')}</th>
                <th style={thStyle}>{t('audit.columnStatus')}</th>
                <th style={thStyle}>{t('audit.columnPolicy')}</th>
                <th style={thStyle}>{t('audit.columnDuration')}</th>
                <th style={thStyle}>{t('audit.columnActions')}</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <>
                  <tr key={entry.id} style={{ borderBottom: '1px solid #f3f4f6' }}>
                    <td style={tdStyle}>
                      <span style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>{formatTime(entry.createdAt)}</span>
                    </td>
                    <td style={tdStyle}>
                      <span style={{
                        fontWeight: 500, fontSize: '12px', padding: '2px 8px',
                        backgroundColor: 'var(--bg-glass)', borderRadius: '4px', fontFamily: 'monospace',
                      }}>
                        {entry.toolName}
                      </span>
                    </td>
                    <td style={tdStyle}>
                      <span style={{
                        fontSize: '11px', padding: '2px 8px', borderRadius: '4px',
                        backgroundColor: entry.success ? 'var(--success-bg)' : 'var(--error-bg)',
                        color: entry.success ? 'var(--success)' : 'var(--error)',
                      }}>
                        {entry.success ? t('audit.statusSuccess') : t('audit.statusFailure')}
                      </span>
                    </td>
                    <td style={tdStyle}>
                      <span style={{ fontSize: '12px', color: 'var(--text-secondary)' }}>
                        {entry.policyDecision}
                      </span>
                    </td>
                    <td style={tdStyle}>
                      <span style={{ fontSize: '12px', color: entry.durationMs > 5000 ? 'var(--error)' : 'var(--text-secondary)' }}>
                        {entry.durationMs}ms
                      </span>
                    </td>
                    <td style={tdStyle}>
                      <button
                        onClick={() => setExpandedId(expandedId === entry.id ? null : entry.id)}
                        style={{
                          fontSize: '11px', padding: '2px 8px', border: '1px solid var(--border-subtle)',
                          borderRadius: '4px', backgroundColor: 'var(--bg-elevated)', cursor: 'pointer',
                        }}
                      >
                        {expandedId === entry.id ? t('common.collapse') : t('common.details')}
                      </button>
                    </td>
                  </tr>
                  {expandedId === entry.id && (
                    <tr key={`${entry.id}-detail`}>
                      <td colSpan={6} style={{ padding: '12px 16px', backgroundColor: 'var(--bg-glass)' }}>
                        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                          <div>
                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '4px' }}>{t('audit.labelParams')}</div>
                            <pre style={{
                              margin: 0, padding: '8px', backgroundColor: '#1e1e1e', color: '#d4d4d4',
                              borderRadius: '6px', fontSize: '11px', overflow: 'auto', maxHeight: '200px',
                            }}>
                              {formatJson(entry.arguments)}
                            </pre>
                          </div>
                          <div>
                            <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '4px' }}>{t('audit.labelResult')}</div>
                            <pre style={{
                              margin: 0, padding: '8px', backgroundColor: '#1e1e1e', color: '#d4d4d4',
                              borderRadius: '6px', fontSize: '11px', overflow: 'auto', maxHeight: '200px',
                            }}>
                              {entry.result ? (entry.result.length > 500 ? entry.result.substring(0, 500) + '...' : entry.result) : '(无)'}
                            </pre>
                          </div>
                        </div>
                        <div style={{ marginTop: '8px', fontSize: '11px', color: 'var(--text-muted)' }}>
                          {t('audit.labelPolicySource')}: {entry.policySource} | {t('audit.labelSession')}: {entry.sessionId?.substring(0, 8)}...
                        </div>
                      </td>
                    </tr>
                  )}
                </>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* 分页 */}
      <div style={{ display: 'flex', justifyContent: 'center', gap: '12px', marginTop: '16px' }}>
        <button
          onClick={() => setPage(Math.max(0, page - 1))}
          disabled={page === 0}
          style={{ ...pageBtnStyle, opacity: page === 0 ? 0.4 : 1 }}
        >
          {t('common.prevPage')}
        </button>
        <span style={{ fontSize: '13px', color: 'var(--text-secondary)', lineHeight: '32px' }}>{t('common.page')}{page + 1}{t('common.pageSuffix')}</span>
        <button
          onClick={() => setPage(page + 1)}
          disabled={entries.length < PAGE_SIZE}
          style={{ ...pageBtnStyle, opacity: entries.length < PAGE_SIZE ? 0.4 : 1 }}
        >
          {t('common.nextPage')}
        </button>
      </div>
    </div>
  )
}

const thStyle: React.CSSProperties = {
  padding: '10px 12px', textAlign: 'left', fontSize: '12px', fontWeight: 600,
  color: 'var(--text-secondary)', borderBottom: '1px solid var(--border-subtle)',
}

const tdStyle: React.CSSProperties = {
  padding: '8px 12px', verticalAlign: 'middle',
}

const pageBtnStyle: React.CSSProperties = {
  padding: '6px 16px', fontSize: '13px', border: '1px solid var(--border-subtle)',
  borderRadius: '6px', backgroundColor: 'var(--bg-elevated)', cursor: 'pointer',
}

function formatTime(ts: number): string {
  if (!ts) return ''
  const d = new Date(ts)
  return d.toLocaleString('zh-CN', {
    month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit',
  })
}

function formatJson(str: string): string {
  try {
    return JSON.stringify(JSON.parse(str), null, 2)
  } catch {
    return str || '(无)'
  }
}
