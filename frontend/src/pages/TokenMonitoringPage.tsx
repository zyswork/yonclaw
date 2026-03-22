/**
 * Token 监控页面
 *
 * 展示实际 LLM API 调用的 token 消耗统计
 * 数据来源：本地 SQLite token_usage 表
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface ModelStats {
  model: string
  input_tokens: number
  output_tokens: number
  total_tokens: number
  calls: number
}

interface AgentTokenStats {
  agent_id: string
  days: number
  total_input_tokens: number
  total_output_tokens: number
  total_tokens: number
  models: ModelStats[]
}

interface DailyStats {
  date: string
  inputTokens: number
  outputTokens: number
  totalTokens: number
  calls: number
}

interface Agent {
  id: string
  name: string
  model: string
}

function estimateCost(model: string, input: number, output: number): number {
  const m = model.toLowerCase()
  const [ip, op] = m.includes('claude') ? [3, 15]
    : m.includes('gpt-5') || m.includes('gpt-4o') ? [2.5, 10]
    : m.includes('gpt-4o-mini') ? [0.15, 0.6]
    : m.includes('deepseek') ? [0.14, 0.28]
    : [1, 3]
  return (input * ip + output * op) / 1_000_000
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

export default function TokenMonitoringPage() {
  const { t } = useI18n()
  const [agents, setAgents] = useState<Agent[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [stats, setStats] = useState<AgentTokenStats | null>(null)
  const [daily, setDaily] = useState<DailyStats[]>([])
  const [days, setDays] = useState(7)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    invoke<Agent[]>('list_agents').then((r) => {
      if (Array.isArray(r)) {
        setAgents(r)
        if (r.length > 0) setSelectedAgent(r[0].id)
      }
    }).catch((e) => {
      console.error('list_agents failed:', e)
      setLoading(false)
    })
  }, [])

  useEffect(() => {
    if (!selectedAgent) return
    setLoading(true)
    Promise.all([
      invoke<AgentTokenStats>('get_token_stats', { agentId: selectedAgent, days }),
      invoke<DailyStats[]>('get_token_daily_stats', { agentId: selectedAgent, days }),
    ]).then(([s, d]) => {
      setStats(s)
      setDaily(d || [])
    }).catch(console.error)
      .finally(() => setLoading(false))
  }, [selectedAgent, days])

  const totalCost = stats?.models?.reduce((sum, m) =>
    sum + estimateCost(m.model, m.input_tokens, m.output_tokens), 0) ?? 0
  const maxDaily = Math.max(...daily.map(d => d.totalTokens), 1)

  return (
    <div style={{ padding: 24, maxWidth: 900 }}>
      <h2 style={{ margin: '0 0 20px', fontSize: 20 }}>{t('tokens.title')}</h2>

      <div style={{ display: 'flex', gap: 12, marginBottom: 20 }}>
        <select
          value={selectedAgent}
          onChange={(e) => setSelectedAgent(e.target.value)}
          style={{ padding: '6px 12px', border: '1px solid var(--border-subtle)', borderRadius: 6, fontSize: 14 }}
        >
          {agents.map(a => <option key={a.id} value={a.id}>{a.name} ({a.model})</option>)}
        </select>
        {[7, 14, 30].map(d => (
          <button key={d} onClick={() => setDays(d)} style={{
            padding: '6px 12px', border: `1px solid ${days === d ? 'var(--accent)' : '#ddd'}`,
            borderRadius: 6, backgroundColor: days === d ? 'var(--accent)' : 'white',
            color: days === d ? 'white' : '#333', cursor: 'pointer', fontSize: 13,
          }}>{d}{t('tokens.labelDays')}</button>
        ))}
      </div>

      {loading ? <div style={{ color: 'var(--text-muted)' }}>{t('common.loading')}</div> : stats && (
        <>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 24 }}>
            {[
              { label: t('tokens.statTotal'), value: formatTokens(stats.total_tokens), color: 'var(--accent)' },
              { label: t('tokens.statInput'), value: formatTokens(stats.total_input_tokens), color: 'var(--success)' },
              { label: t('tokens.statOutput'), value: formatTokens(stats.total_output_tokens), color: '#fd7e14' },
              { label: t('tokens.statCost'), value: `$${totalCost.toFixed(2)}`, color: 'var(--error)' },
            ].map(({ label, value, color }) => (
              <div key={label} style={{
                padding: 16, borderRadius: 8, border: '1px solid var(--border-subtle)', textAlign: 'center',
              }}>
                <div style={{ fontSize: 24, fontWeight: 700, color }}>{value}</div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 4 }}>{label}</div>
              </div>
            ))}
          </div>

          {daily.length > 0 && (
            <div style={{ marginBottom: 24 }}>
              <h3 style={{ fontSize: 15, margin: '0 0 12px' }}>{t('tokens.sectionDailyTrend')}</h3>
              <div style={{ display: 'flex', alignItems: 'flex-end', gap: 2, height: 120, padding: '0 4px' }}>
                {daily.map((d, i) => (
                  <div key={i} style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center' }}>
                    <div style={{ fontSize: 10, color: 'var(--text-muted)', marginBottom: 2 }}>{formatTokens(d.totalTokens)}</div>
                    <div style={{
                      width: '100%', maxWidth: 30,
                      height: `${(d.totalTokens / maxDaily) * 80}px`,
                      backgroundColor: 'var(--accent)', borderRadius: '3px 3px 0 0', minHeight: 2,
                    }} title={`${d.date}: ${d.totalTokens} tokens, ${d.calls} calls`} />
                    <div style={{ fontSize: 9, color: '#bbb', marginTop: 2 }}>{d.date.slice(5)}</div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {stats.models.length > 0 && (
            <div>
              <h3 style={{ fontSize: 15, margin: '0 0 12px' }}>{t('tokens.sectionModels')}</h3>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
                <thead>
                  <tr style={{ borderBottom: '2px solid #e5e7eb' }}>
                    <th style={{ textAlign: 'left', padding: '8px 12px' }}>{t('tokens.columnModel')}</th>
                    <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('tokens.columnInput')}</th>
                    <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('tokens.columnOutput')}</th>
                    <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('tokens.columnTotal')}</th>
                    <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('tokens.columnCalls')}</th>
                    <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('tokens.columnCost')}</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.models.map((m) => (
                    <tr key={m.model} style={{ borderBottom: '1px solid #f0f0f0' }}>
                      <td style={{ padding: '8px 12px', fontFamily: 'monospace' }}>{m.model}</td>
                      <td style={{ padding: '8px 12px', textAlign: 'right' }}>{formatTokens(m.input_tokens)}</td>
                      <td style={{ padding: '8px 12px', textAlign: 'right' }}>{formatTokens(m.output_tokens)}</td>
                      <td style={{ padding: '8px 12px', textAlign: 'right', fontWeight: 600 }}>{formatTokens(m.total_tokens)}</td>
                      <td style={{ padding: '8px 12px', textAlign: 'right' }}>{m.calls}</td>
                      <td style={{ padding: '8px 12px', textAlign: 'right', color: 'var(--error)' }}>
                        ${estimateCost(m.model, m.input_tokens, m.output_tokens).toFixed(4)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  )
}
