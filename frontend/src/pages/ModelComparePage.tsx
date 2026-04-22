import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast, friendlyError } from '../hooks/useToast'
import { prettyModel, buildProviderNameMap } from '../utils/pretty-model'

/**
 * 并列对比：同一 prompt 并行发给两个模型，直观看回答差异。
 *
 * 实现简化版：两次 `ai_generate_agent_config` 样式调用通过 `cloud_api_proxy`
 * 或复用 cookie 里的现有 provider 发起请求。当前只做 UI 骨架 + 串行 invoke。
 */
export default function ModelComparePage() {
  const { t } = useI18n()
  const [agents, setAgents] = useState<Array<{ id: string; name: string; model: string }>>([])
  const [providerMap, setProviderMap] = useState<Record<string, string>>({})
  const [agentA, setAgentA] = useState('')
  const [agentB, setAgentB] = useState('')
  const [prompt, setPrompt] = useState('')
  const [replyA, setReplyA] = useState('')
  const [replyB, setReplyB] = useState('')
  const [loading, setLoading] = useState<'idle' | 'a' | 'b' | 'both'>('idle')

  useEffect(() => {
    Promise.all([
      invoke<Array<{ id: string; name: string; model: string }>>('list_agents').catch(() => []),
      invoke<Array<{ id?: string; name?: string }>>('get_providers').catch(() => []),
    ]).then(([a, p]) => {
      setAgents(a || [])
      setProviderMap(buildProviderNameMap(p || []))
      if (a && a.length >= 1) setAgentA(a[0].id)
      if (a && a.length >= 2) setAgentB(a[1].id)
    })
  }, [])

  const runOne = async (agentId: string, text: string): Promise<string> => {
    // 创建一次性会话 → send → 读最新 assistant 回复
    const sid = `cli-infer-${Date.now()}`
    await invoke('send_message', { agentId, sessionId: sid, message: text })
    const msgs = await invoke<any[]>('get_session_messages', { agentId, sessionId: sid })
    const lastAssistant = (msgs || []).reverse().find((m: any) => m.role === 'assistant')
    return lastAssistant?.content || '（无回复）'
  }

  const runBoth = async () => {
    if (!prompt.trim() || !agentA || !agentB) {
      toast.error('请选择两个 Agent 并输入问题')
      return
    }
    setLoading('both')
    setReplyA(''); setReplyB('')
    try {
      const [a, b] = await Promise.all([
        runOne(agentA, prompt).catch((e) => `⚠️ ${friendlyError(e)}`),
        runOne(agentB, prompt).catch((e) => `⚠️ ${friendlyError(e)}`),
      ])
      setReplyA(a)
      setReplyB(b)
    } finally {
      setLoading('idle')
    }
  }

  const agentA_obj = agents.find(a => a.id === agentA)
  const agentB_obj = agents.find(a => a.id === agentB)

  return (
    <div style={{ padding: 24, maxWidth: 1200 }}>
      <h1 style={{ margin: '0 0 16px', fontSize: 22 }}>并列对比</h1>
      <p style={{ fontSize: 13, color: 'var(--text-muted)', margin: '0 0 16px' }}>
        同一问题并行发给两个 Agent，比较回答差异、速度、质量。
      </p>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginBottom: 12 }}>
        {[{ label: 'Agent A', id: agentA, set: setAgentA, obj: agentA_obj },
          { label: 'Agent B', id: agentB, set: setAgentB, obj: agentB_obj }].map(({ label, id, set, obj }) => (
          <div key={label}>
            <label style={{ fontSize: 12, color: 'var(--text-muted)', display: 'block', marginBottom: 4 }}>{label}</label>
            <select value={id} onChange={e => set(e.target.value)} style={{
              width: '100%', padding: '8px 10px', borderRadius: 6,
              border: '1px solid var(--border-subtle)', fontSize: 13,
              background: 'var(--bg-elevated)', color: 'var(--text-primary)',
            }}>
              <option value="">— 选择 —</option>
              {agents.map(a => (
                <option key={a.id} value={a.id}>{a.name} · {prettyModel(a.model, providerMap)}</option>
              ))}
            </select>
          </div>
        ))}
      </div>

      <textarea
        value={prompt}
        onChange={e => setPrompt(e.target.value)}
        placeholder="输入要对比的问题..."
        rows={4}
        style={{
          width: '100%', padding: 10, borderRadius: 6, border: '1px solid var(--border-subtle)',
          fontSize: 14, fontFamily: 'inherit', resize: 'vertical', boxSizing: 'border-box',
        }}
      />
      <button onClick={runBoth} disabled={loading !== 'idle'} style={{
        marginTop: 8, padding: '8px 20px', border: 'none', borderRadius: 6,
        background: 'var(--accent)', color: '#fff', cursor: loading !== 'idle' ? 'wait' : 'pointer',
        fontSize: 13, fontWeight: 500,
      }}>
        {loading === 'idle' ? '并行发送' : '生成中...'}
      </button>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginTop: 20 }}>
        {[{ title: agentA_obj?.name || 'Agent A', model: agentA_obj?.model, reply: replyA },
          { title: agentB_obj?.name || 'Agent B', model: agentB_obj?.model, reply: replyB }].map((col, i) => (
          <div key={i} style={{
            border: '1px solid var(--border-subtle)', borderRadius: 8, background: 'var(--bg-elevated)',
            padding: 14, minHeight: 300,
          }}>
            <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 6, fontWeight: 500 }}>
              {col.title}
              {col.model && <span style={{ marginLeft: 6, fontSize: 10, padding: '1px 6px', background: 'var(--bg-glass)', borderRadius: 4 }}>
                {prettyModel(col.model, providerMap)}
              </span>}
            </div>
            <div style={{ fontSize: 13, whiteSpace: 'pre-wrap', color: 'var(--text-primary)', lineHeight: 1.6 }}>
              {col.reply || <span style={{ color: 'var(--text-muted)' }}>等待回答...</span>}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
