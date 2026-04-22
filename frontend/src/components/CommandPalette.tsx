import { useEffect, useRef, useState, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/tauri'
import { getRecent } from '../utils/recent'

interface CommandItem {
  id: string
  title: string
  subtitle?: string
  action: () => void
  group: string
}

interface SearchHit {
  sessionId: string
  agentId: string
  seq: number
  role: string
  preview: string
  sessionTitle?: string
}

/**
 * Cmd+K / Ctrl+K 全局命令面板
 *
 * 统一入口：跳转页面、搜索 Agent、执行常用操作。
 * 基于简单的子串匹配（大小写不敏感）。
 */
export default function CommandPalette() {
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState(0)
  const [agents, setAgents] = useState<Array<{ id: string; name: string; model: string }>>([])
  const [msgHits, setMsgHits] = useState<SearchHit[]>([])
  const inputRef = useRef<HTMLInputElement>(null)

  // 快捷键监听
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setOpen(o => !o)
      } else if (e.key === 'Escape' && open) {
        setOpen(false)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open])

  // 打开时加载 agents 并聚焦
  useEffect(() => {
    if (!open) return
    setQuery('')
    setSelected(0)
    setMsgHits([])
    requestAnimationFrame(() => inputRef.current?.focus())
    invoke<Array<{ id: string; name: string; model: string }>>('list_agents')
      .then(setAgents)
      .catch(() => {})
  }, [open])

  // 输入 2+ 字符时触发跨会话搜索（debounce 250ms）
  useEffect(() => {
    if (!open) return
    if (query.trim().length < 2) { setMsgHits([]); return }
    const q = query.trim()
    const timer = setTimeout(() => {
      invoke<SearchHit[]>('search_all_messages', { query: q, limit: 15 })
        .then(r => setMsgHits(Array.isArray(r) ? r : []))
        .catch(() => setMsgHits([]))
    }, 250)
    return () => clearTimeout(timer)
  }, [query, open])

  const items = useMemo<CommandItem[]>(() => {
    const navItems: CommandItem[] = [
      { id: 'nav-dashboard', title: '仪表板', subtitle: '总览 / Model Auth', group: 'Navigation', action: () => navigate('/') },
      { id: 'nav-agents', title: 'Agent 列表', subtitle: '全部 Agent', group: 'Navigation', action: () => navigate('/agents') },
      { id: 'nav-new-agent', title: '新建 Agent', subtitle: '创建新助手', group: 'Navigation', action: () => navigate('/agents/new') },
      { id: 'nav-skills', title: '技能', subtitle: '管理技能', group: 'Navigation', action: () => navigate('/skills') },
      { id: 'nav-cron', title: '定时任务', subtitle: 'Cron 调度', group: 'Navigation', action: () => navigate('/cron') },
      { id: 'nav-memory', title: '记忆', subtitle: 'Memory 管理', group: 'Navigation', action: () => navigate('/memory') },
      { id: 'nav-plugins', title: '插件', subtitle: 'Plugin Manager', group: 'Navigation', action: () => navigate('/plugins') },
      { id: 'nav-channels', title: '频道', subtitle: '消息渠道', group: 'Navigation', action: () => navigate('/channels') },
      { id: 'nav-group', title: '群聊', subtitle: '多 Agent 对话', group: 'Navigation', action: () => navigate('/group-chat') },
      { id: 'nav-plaza', title: '广场', subtitle: '发现 / 分享', group: 'Navigation', action: () => navigate('/plaza') },
      { id: 'nav-audit', title: '审计日志', group: 'Navigation', action: () => navigate('/audit') },
      { id: 'nav-tokens', title: 'Token 监控', group: 'Navigation', action: () => navigate('/tokens') },
      { id: 'nav-doctor', title: '系统诊断', group: 'Navigation', action: () => navigate('/doctor') },
      { id: 'nav-compare', title: '并列对比', subtitle: '同 prompt 对比两个 Agent', group: 'Navigation', action: () => navigate('/compare') },
      { id: 'nav-settings', title: '设置', subtitle: 'Providers / TTS / 备份', group: 'Navigation', action: () => navigate('/settings') },
    ]
    // 最近访问（最多 5 个）
    const recentItems: CommandItem[] = getRecent('agent', 5).map(r => ({
      id: `recent-agent-${r.id}`,
      title: r.name,
      subtitle: '刚刚访问过',
      group: 'Recent',
      action: () => navigate(`/agents/${r.id}`),
    }))
    const agentItems: CommandItem[] = agents.map(a => ({
      id: `agent-${a.id}`,
      title: a.name,
      subtitle: a.model,
      group: 'Agents',
      action: () => navigate(`/agents/${a.id}`),
    }))
    // 跨会话搜索结果（仅当有查询）
    const searchItems: CommandItem[] = msgHits.map(h => ({
      id: `msg-${h.sessionId}-${h.seq}`,
      title: h.preview.slice(0, 80),
      subtitle: `${h.sessionTitle || h.sessionId.slice(0, 8)} · ${h.role}`,
      group: 'Messages',
      action: () => navigate(`/agents/${h.agentId}?session=${h.sessionId}&seq=${h.seq}`),
    }))
    return [...recentItems, ...navItems, ...agentItems, ...searchItems]
  }, [agents, msgHits, navigate])

  const filtered = useMemo(() => {
    if (!query.trim()) return items
    const q = query.toLowerCase()
    return items.filter(i =>
      i.title.toLowerCase().includes(q) ||
      (i.subtitle || '').toLowerCase().includes(q)
    )
  }, [items, query])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelected(s => Math.min(s + 1, filtered.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelected(s => Math.max(s - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const item = filtered[selected]
      if (item) {
        item.action()
        setOpen(false)
      }
    }
  }

  if (!open) return null

  return (
    <div
      role="dialog"
      aria-label="Command palette"
      onClick={() => setOpen(false)}
      style={{
        position: 'fixed', inset: 0, zIndex: 10000,
        backgroundColor: 'rgba(0,0,0,0.45)',
        display: 'flex', justifyContent: 'center', alignItems: 'flex-start', paddingTop: 120,
        backdropFilter: 'blur(4px)',
      }}
    >
      <div
        onClick={e => e.stopPropagation()}
        style={{
          width: 'min(560px, 92vw)',
          backgroundColor: 'var(--bg-elevated)',
          border: '1px solid var(--border-subtle)',
          borderRadius: 12,
          boxShadow: '0 16px 48px rgba(0,0,0,0.5)',
          overflow: 'hidden',
        }}
      >
        <input
          ref={inputRef}
          value={query}
          onChange={e => { setQuery(e.target.value); setSelected(0) }}
          onKeyDown={handleKeyDown}
          placeholder="输入页面、Agent 名称或命令…"
          style={{
            width: '100%', padding: '16px 18px', fontSize: 15,
            border: 'none', outline: 'none',
            backgroundColor: 'transparent', color: 'var(--text-primary)',
            borderBottom: '1px solid var(--border-subtle)',
          }}
        />
        <div style={{ maxHeight: 360, overflowY: 'auto', padding: '6px 0' }}>
          {filtered.length === 0 ? (
            <div style={{ padding: '20px 18px', color: 'var(--text-muted)', fontSize: 13, textAlign: 'center' }}>
              无匹配结果
            </div>
          ) : (
            filtered.map((item, i) => (
              <div
                key={item.id}
                onClick={() => { item.action(); setOpen(false) }}
                onMouseEnter={() => setSelected(i)}
                style={{
                  padding: '10px 18px',
                  cursor: 'pointer',
                  backgroundColor: i === selected ? 'var(--bg-glass)' : 'transparent',
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  fontSize: 13,
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: 1 }}>
                  <div style={{ fontWeight: 500, color: 'var(--text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {item.title}
                  </div>
                  {item.subtitle && (
                    <div style={{ fontSize: 11, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {item.subtitle}
                    </div>
                  )}
                </div>
                <span style={{ fontSize: 10, color: 'var(--text-muted)', padding: '2px 6px', borderRadius: 4, backgroundColor: 'var(--bg-glass)', marginLeft: 12 }}>
                  {item.group}
                </span>
              </div>
            ))
          )}
        </div>
        <div style={{
          padding: '8px 18px', fontSize: 11, color: 'var(--text-muted)',
          borderTop: '1px solid var(--border-subtle)', display: 'flex', gap: 14,
          backgroundColor: 'var(--bg-glass)',
        }}>
          <span>↑↓ 选择</span>
          <span>⏎ 执行</span>
          <span>Esc 关闭</span>
          <span style={{ marginLeft: 'auto' }}>⌘K 切换</span>
        </div>
      </div>
    </div>
  )
}
