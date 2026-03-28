/**
 * 工具管理 Tab
 *
 * 功能：
 * - 工具配置文件选择（基础/编程/完整）- pill 样式分类 tab
 * - 工具列表展示（名称、描述、安全等级、开关）
 * - 内置工具和 MCP 工具分组显示
 * - 搜索框过滤工具
 * - 工具卡片 hover 微上浮 + 左侧类型图标
 * - 自定义 toggle switch
 */

import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface ToolsTabProps {
  agentId: string
}

interface ToolInfo {
  name: string
  description: string
  safety: string
  enabled: boolean
  source: string
}

interface AgentTools {
  profile: string
  tools: ToolInfo[]
}

/** 安全等级颜色映射 */
const SAFETY_COLORS: Record<string, { bg: string; color: string; border: string }> = {
  safe:      { bg: 'rgba(34,197,94,0.12)', color: '#22c55e', border: 'rgba(34,197,94,0.25)' },
  guarded:   { bg: 'rgba(245,158,11,0.12)', color: '#f59e0b', border: 'rgba(245,158,11,0.25)' },
  sandboxed: { bg: 'rgba(249,115,22,0.12)', color: '#f97316', border: 'rgba(249,115,22,0.25)' },
  approval:  { bg: 'rgba(239,68,68,0.12)', color: '#ef4444', border: 'rgba(239,68,68,0.25)' },
}

/** 根据工具名称推断图标类型 */
function getToolIcon(name: string): string {
  const n = name.toLowerCase()
  if (n.includes('file') || n.includes('read') || n.includes('write') || n.includes('edit'))
    return 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M16 13H8 M16 17H8 M10 9H8'
  if (n.includes('search') || n.includes('grep') || n.includes('glob') || n.includes('find'))
    return 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35'
  if (n.includes('bash') || n.includes('exec') || n.includes('command') || n.includes('shell') || n.includes('terminal'))
    return 'M4 17l6-6-6-6 M12 19h8'
  if (n.includes('web') || n.includes('http') || n.includes('fetch') || n.includes('url') || n.includes('browser'))
    return 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M2 12h20 M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z'
  if (n.includes('git') || n.includes('commit') || n.includes('branch'))
    return 'M6 3v12 M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M18 9a9 9 0 0 1-9 9'
  if (n.includes('notebook') || n.includes('jupyter'))
    return 'M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z'
  if (n.includes('mcp') || n.includes('plugin') || n.includes('server'))
    return 'M12 2L2 7l10 5 10-5-10-5z M2 17l10 5 10-5 M2 12l10 5 10-5'
  if (n.includes('image') || n.includes('screenshot') || n.includes('visual'))
    return 'M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z'
  if (n.includes('agent') || n.includes('worktree'))
    return 'M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2 M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z M23 21v-2a4 4 0 0 0-3-3.87 M16 3.13a4 4 0 0 1 0 7.75'
  // 默认：齿轮/工具图标
  return 'M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z'
}

const PROFILE_IDS = ['basic', 'coding', 'full'] as const

/** 分类 tab 类型 */
type CategoryTab = 'all' | 'builtin' | 'mcp'

export default function ToolsTab({ agentId }: ToolsTabProps) {
  const { t } = useI18n()

  const PROFILES = PROFILE_IDS.map(id => ({
    id,
    label: t(`toolsTab.profile${id.charAt(0).toUpperCase() + id.slice(1)}` as 'toolsTab.profileBasic'),
  }))

  const [profile, setProfile] = useState('')
  const [tools, setTools] = useState<ToolInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [status, setStatus] = useState('')
  const [searchQuery, setSearchQuery] = useState('')
  const [categoryTab, setCategoryTab] = useState<CategoryTab>('all')

  const loadTools = useCallback(async () => {
    setLoading(true)
    try {
      const result = await invoke<AgentTools>('get_agent_tools', { agentId })
      setProfile(result.profile || 'basic')
      setTools(result.tools || [])
    } catch (e) {
      console.error('加载工具列表失败:', e)
    } finally {
      setLoading(false)
    }
  }, [agentId])

  useEffect(() => {
    loadTools()
  }, [loadTools])

  /** 切换工具配置文件 */
  const handleSetProfile = async (newProfile: string) => {
    try {
      await invoke('set_agent_tool_profile', { agentId, profile: newProfile })
      setStatus(t('toolsTab.switched'))
      setTimeout(() => setStatus(''), 1500)
      await loadTools()
    } catch (e) {
      setStatus(t('toolsTab.switchFailed') + ': ' + String(e))
    }
  }

  /** 切换单个工具开关 */
  const handleToggleTool = async (toolName: string, enabled: boolean) => {
    // 乐观更新
    setTools(prev => prev.map(t => t.name === toolName ? { ...t, enabled } : t))
    try {
      await invoke('set_agent_tool_override', { agentId, toolName, enabled })
    } catch (e) {
      // 回滚
      setTools(prev => prev.map(t => t.name === toolName ? { ...t, enabled: !enabled } : t))
      setStatus(t('toolsTab.operationFailed') + ': ' + String(e))
    }
  }

  const builtinTools = tools.filter(t => t.source === 'builtin')
  const mcpTools = tools.filter(t => t.source !== 'builtin')
  const isCustom = !PROFILES.some(p => p.id === profile)

  // 按搜索和分类过滤
  const filteredTools = tools.filter(t => {
    const matchSearch = !searchQuery ||
      t.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      t.description.toLowerCase().includes(searchQuery.toLowerCase())
    const matchCategory =
      categoryTab === 'all' ? true :
      categoryTab === 'builtin' ? t.source === 'builtin' :
      t.source !== 'builtin'
    return matchSearch && matchCategory
  })

  if (loading) {
    return <div style={{ padding: '20px', textAlign: 'center', color: 'var(--text-muted)', fontSize: '13px' }}>{t('common.loading')}</div>
  }

  const categoryTabs: { id: CategoryTab; label: string; count: number }[] = [
    { id: 'all', label: t('toolsTab.allTools' as 'toolsTab.builtinTools') || 'All', count: tools.length },
    { id: 'builtin', label: t('toolsTab.builtinTools'), count: builtinTools.length },
    { id: 'mcp', label: t('toolsTab.mcpTools'), count: mcpTools.length },
  ]

  return (
    <div style={{ padding: '8px 0' }}>
      {/* 配置文件选择 */}
      <div style={{ marginBottom: '14px' }}>
        <div style={{ fontSize: '12px', color: 'var(--text-secondary)', marginBottom: '6px' }}>{t('toolsTab.toolConfig')}</div>
        <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
          {PROFILES.map(p => (
            <button
              key={p.id}
              onClick={() => handleSetProfile(p.id)}
              style={{
                padding: '5px 14px', fontSize: '12px', border: 'none',
                borderRadius: '9999px', cursor: 'pointer',
                backgroundColor: profile === p.id ? 'var(--accent)' : 'rgba(255,255,255,0.06)',
                color: profile === p.id ? 'white' : 'var(--text-secondary)',
                fontWeight: profile === p.id ? 600 : 400,
                transition: 'all 0.2s ease',
                boxShadow: profile === p.id ? '0 2px 8px rgba(99,102,241,0.3)' : 'none',
              }}
            >
              {p.label}
            </button>
          ))}
          {isCustom && (
            <span style={{ fontSize: '11px', color: 'var(--text-muted)', marginLeft: '4px' }}>{t('toolsTab.custom')}</span>
          )}
        </div>
      </div>

      {status && (
        <div style={{
          fontSize: '12px', marginBottom: '8px', textAlign: 'center',
          color: (status.includes(t('toolsTab.switchFailed')) || status.includes(t('toolsTab.operationFailed'))) ? 'var(--error)' : 'var(--success)',
        }}>
          {status}
        </div>
      )}

      {/* 搜索框 */}
      <div style={{ position: 'relative', marginBottom: '12px' }}>
        <svg
          width="14" height="14" viewBox="0 0 24 24" fill="none"
          stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ position: 'absolute', left: '10px', top: '50%', transform: 'translateY(-50%)', pointerEvents: 'none' }}
        >
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input
          type="text"
          placeholder={t('toolsTab.searchPlaceholder' as 'toolsTab.builtinTools') || 'Search tools...'}
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          style={{
            width: '100%', padding: '7px 10px 7px 32px', fontSize: '12px',
            border: '1px solid var(--border-subtle)', borderRadius: '8px',
            backgroundColor: 'var(--bg-elevated)', color: 'var(--text-primary)',
            outline: 'none', boxSizing: 'border-box',
          }}
        />
      </div>

      {/* 分类 pill tabs */}
      <div style={{
        display: 'flex', gap: '4px', marginBottom: '12px',
        padding: '3px', backgroundColor: 'rgba(255,255,255,0.04)', borderRadius: '10px',
      }}>
        {categoryTabs.map(tab => (
          <button
            key={tab.id}
            onClick={() => setCategoryTab(tab.id)}
            style={{
              flex: 1, padding: '5px 10px', fontSize: '11px', border: 'none',
              borderRadius: '8px', cursor: 'pointer',
              backgroundColor: categoryTab === tab.id ? 'var(--accent)' : 'transparent',
              color: categoryTab === tab.id ? 'white' : 'var(--text-muted)',
              fontWeight: categoryTab === tab.id ? 600 : 400,
              transition: 'all 0.2s ease',
            }}
          >
            {tab.label} ({tab.count})
          </button>
        ))}
      </div>

      {/* 工具列表 */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
        {filteredTools.map(tool => (
          <ToolRow key={tool.name} tool={tool} onToggle={handleToggleTool} />
        ))}
      </div>

      {filteredTools.length === 0 && (
        <div style={{ textAlign: 'center', color: 'var(--text-muted)', fontSize: '13px', padding: '20px 0' }}>
          {searchQuery ? (t('toolsTab.noSearchResults' as 'toolsTab.noTools') || 'No tools match your search') : t('toolsTab.noTools')}
        </div>
      )}
    </div>
  )
}

/** 单个工具行组件 */
function ToolRow({ tool, onToggle }: { tool: ToolInfo; onToggle: (name: string, enabled: boolean) => void }) {
  const { t } = useI18n()
  const [hovered, setHovered] = useState(false)
  const safetyKey = tool.safety?.toLowerCase() || 'safe'
  const colors = SAFETY_COLORS[safetyKey] || SAFETY_COLORS.safe
  const safetyLabels: Record<string, string> = {
    safe: t('toolsTab.safetySafe'),
    guarded: t('toolsTab.safetyGuarded'),
    sandboxed: t('toolsTab.safetySandboxed'),
    approval: t('toolsTab.safetyApproval'),
  }
  const label = safetyLabels[safetyKey] || tool.safety
  const iconPath = getToolIcon(tool.name)

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex', alignItems: 'center', padding: '8px 10px',
        borderRadius: '8px',
        backgroundColor: hovered ? 'rgba(255,255,255,0.06)' : 'var(--bg-glass)',
        gap: '10px',
        transition: 'all 0.2s ease',
        transform: hovered ? 'translateY(-1px)' : 'none',
        boxShadow: hovered ? '0 4px 12px rgba(0,0,0,0.12)' : 'none',
        cursor: 'default',
      }}
    >
      {/* 工具类型图标 */}
      <div style={{
        width: '30px', height: '30px', borderRadius: '7px',
        backgroundColor: 'rgba(99,102,241,0.1)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        flexShrink: 0,
      }}>
        <svg
          width="15" height="15" viewBox="0 0 24 24" fill="none"
          stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
        >
          <path d={iconPath} />
        </svg>
      </div>

      {/* 工具信息 */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: '13px', fontWeight: 500, color: 'var(--text-primary)' }}>{tool.name}</div>
        {tool.description && (
          <div style={{
            fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
            {tool.description}
          </div>
        )}
      </div>

      {/* 安全等级标签 */}
      <span style={{
        fontSize: '10px', padding: '2px 8px', borderRadius: '9999px',
        backgroundColor: colors.bg, color: colors.color,
        border: `1px solid ${colors.border}`,
        whiteSpace: 'nowrap', fontWeight: 500,
      }}>
        {label}
      </span>

      {/* 自定义 toggle switch */}
      <button
        role="switch"
        aria-checked={tool.enabled}
        onClick={() => onToggle(tool.name, !tool.enabled)}
        style={{
          position: 'relative', width: '36px', height: '20px', flexShrink: 0,
          border: 'none', borderRadius: '10px', cursor: 'pointer', padding: 0,
          backgroundColor: tool.enabled ? 'var(--accent)' : 'var(--border-subtle)',
          transition: 'background-color 0.2s ease',
          outline: 'none',
        }}
      >
        <span style={{
          position: 'absolute', height: '16px', width: '16px',
          left: tool.enabled ? '18px' : '2px', top: '2px',
          backgroundColor: 'white', borderRadius: '50%',
          transition: 'left 0.2s ease',
          boxShadow: '0 1px 3px rgba(0,0,0,0.2)',
        }} />
      </button>
    </div>
  )
}
