/**
 * Token 监控页面
 *
 * 展示实际 LLM API 调用的 token 消耗统计
 * 数据来源：本地 SQLite token_usage 表
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import Select from '../components/Select'

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
  const [ip, op] =
    // GPT-5.x
    m.includes('gpt-5') && m.includes('mini') ? [0.30, 1.20]
    : m.includes('gpt-5') || m.includes('gpt-4.5') ? [5, 20]
    // GPT-4.x
    : m.includes('gpt-4o-mini') || m.includes('gpt-4.1-mini') ? [0.15, 0.6]
    : m.includes('gpt-4o') || m.includes('gpt-4.1') ? [2.5, 10]
    // o 系列
    : m.includes('o4-mini') || m.includes('o3-mini') ? [1.1, 4.4]
    : m.includes('o3') || m.includes('o4') ? [10, 40]
    // Claude 4.x
    : m.includes('claude-opus-4') ? [15, 75]
    : m.includes('claude-sonnet-4') ? [3, 15]
    : m.includes('claude-haiku-4') ? [0.8, 4]
    : m.includes('claude-opus') ? [15, 75]
    : m.includes('claude-sonnet') ? [3, 15]
    : m.includes('claude-haiku') ? [0.25, 1.25]
    : m.includes('claude') ? [3, 15]
    // Gemini
    : m.includes('gemini') && m.includes('flash') ? [0.075, 0.3]
    : m.includes('gemini') && m.includes('pro') ? [1.25, 5]
    : m.includes('gemini') ? [0.5, 1.5]
    // DeepSeek
    : m.includes('deepseek-r1') ? [0.55, 2.19]
    : m.includes('deepseek') ? [0.27, 1.1]
    // Grok
    : m.includes('grok') && m.includes('mini') ? [0.3, 0.5]
    : m.includes('grok') ? [3, 15]
    // Qwen
    : m.includes('qwen') && m.includes('turbo') ? [0.3, 0.6]
    : m.includes('qwen') ? [0.8, 2]
    // Others
    : m.includes('moonshot') || m.includes('kimi') ? [1, 1]
    : m.includes('glm') && m.includes('flash') ? [0.1, 0.1]
    : m.includes('glm') ? [1, 1]
    : m.includes('mistral') && m.includes('large') ? [2, 6]
    : m.includes('mistral') ? [0.25, 0.25]
    : m.includes('llama') ? [0.2, 0.2]
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
        <Select
          value={selectedAgent}
          onChange={setSelectedAgent}
          options={agents.map(a => ({ value: a.id, label: `${a.name} (${a.model})` }))}
          style={{ minWidth: 200 }}
        />
        {[7, 14, 30].map(d => (
          <button key={d} onClick={() => setDays(d)} style={{
            padding: '6px 12px', border: `1px solid ${days === d ? 'var(--accent)' : '#ddd'}`,
            borderRadius: 6, backgroundColor: days === d ? 'var(--accent)' : 'var(--bg-elevated)',
            color: days === d ? '#fff' : 'var(--text-primary)', cursor: 'pointer', fontSize: 13,
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
                    <div style={{ fontSize: 9, color: 'var(--text-muted)', marginTop: 2 }}>{d.date.slice(5)}</div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* 模型使用分布（饼图效果 - 水平条） */}
          {stats.models.length > 1 && (
            <div style={{ marginBottom: 24 }}>
              <h3 style={{ fontSize: 15, margin: '0 0 12px' }}>Model Distribution</h3>
              <div style={{ display: 'flex', height: 24, borderRadius: 6, overflow: 'hidden', border: '1px solid var(--border-subtle)' }}>
                {(() => {
                  const total = stats.models.reduce((s: number, m: any) => s + m.total_tokens, 0)
                  const colors = ['var(--accent)', '#fd7e14', 'var(--success)', '#6f42c1', '#e83e8c', '#20c997', '#6c757d']
                  return stats.models.map((m: any, i: number) => {
                    const pct = total > 0 ? (m.total_tokens / total * 100) : 0
                    return pct > 0 ? (
                      <div key={m.model} title={`${m.model}: ${pct.toFixed(1)}%`}
                        style={{ width: `${pct}%`, backgroundColor: colors[i % colors.length], minWidth: pct > 3 ? undefined : 2 }}>
                        {pct > 8 && <span style={{ fontSize: 10, color: '#fff', padding: '0 4px', lineHeight: '24px', whiteSpace: 'nowrap', overflow: 'hidden' }}>{m.model.split('/').pop()}</span>}
                      </div>
                    ) : null
                  })
                })()}
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px 16px', marginTop: 8 }}>
                {(() => {
                  const colors = ['var(--accent)', '#fd7e14', 'var(--success)', '#6f42c1', '#e83e8c', '#20c997', '#6c757d']
                  const total = stats.models.reduce((s: number, m: any) => s + m.total_tokens, 0)
                  return stats.models.map((m: any, i: number) => (
                    <span key={m.model} style={{ fontSize: 11, display: 'flex', alignItems: 'center', gap: 4 }}>
                      <span style={{ width: 8, height: 8, borderRadius: 2, backgroundColor: colors[i % colors.length], display: 'inline-block' }} />
                      {m.model} ({(m.total_tokens / total * 100).toFixed(1)}%)
                    </span>
                  ))
                })()}
              </div>
            </div>
          )}

          {stats.models.length > 0 && (
            <div>
              <h3 style={{ fontSize: 15, margin: '0 0 12px' }}>{t('tokens.sectionModels')}</h3>
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
                <thead>
                  <tr style={{ borderBottom: '2px solid var(--border-default)' }}>
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
                    <tr key={m.model} style={{ borderBottom: '1px solid var(--border-subtle)' }}>
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
                  {/* 合计行 */}
                  <tr style={{ borderTop: '2px solid var(--border-default)', fontWeight: 700 }}>
                    <td style={{ padding: '8px 12px' }}>{t('tokens.totalRow') || 'Total'}</td>
                    <td style={{ padding: '8px 12px', textAlign: 'right' }}>{formatTokens(stats.models.reduce((s, m) => s + m.input_tokens, 0))}</td>
                    <td style={{ padding: '8px 12px', textAlign: 'right' }}>{formatTokens(stats.models.reduce((s, m) => s + m.output_tokens, 0))}</td>
                    <td style={{ padding: '8px 12px', textAlign: 'right' }}>{formatTokens(stats.models.reduce((s, m) => s + m.total_tokens, 0))}</td>
                    <td style={{ padding: '8px 12px', textAlign: 'right' }}>{stats.models.reduce((s, m) => s + m.calls, 0)}</td>
                    <td style={{ padding: '8px 12px', textAlign: 'right', color: 'var(--error)', fontSize: 14 }}>
                      ${stats.models.reduce((s, m) => s + estimateCost(m.model, m.input_tokens, m.output_tokens), 0).toFixed(4)}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  )
}
