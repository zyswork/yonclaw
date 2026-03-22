/**
 * 工具管理 Tab
 *
 * 功能：
 * - 工具配置文件选择（基础/编程/完整）
 * - 工具列表展示（名称、描述、安全等级、开关）
 * - 内置工具和 MCP 工具分组显示
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
const SAFETY_COLORS: Record<string, { bg: string; color: string }> = {
  safe:     { bg: '#d4edda', color: '#155724' },
  guarded:  { bg: '#fff3cd', color: '#856404' },
  sandboxed: { bg: '#ffe0cc', color: '#c45000' },
  approval: { bg: '#f8d7da', color: '#721c24' },
}

const PROFILE_IDS = ['basic', 'coding', 'full'] as const

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

  if (loading) {
    return <div style={{ padding: '20px', textAlign: 'center', color: '#999', fontSize: '13px' }}>{t('common.loading')}</div>
  }

  return (
    <div style={{ padding: '8px 0' }}>
      {/* 配置文件选择 */}
      <div style={{ marginBottom: '12px' }}>
        <div style={{ fontSize: '12px', color: '#666', marginBottom: '6px' }}>{t('toolsTab.toolConfig')}</div>
        <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
          {PROFILES.map(p => (
            <button
              key={p.id}
              onClick={() => handleSetProfile(p.id)}
              style={{
                padding: '4px 12px', fontSize: '12px', border: '1px solid',
                borderColor: profile === p.id ? 'var(--accent)' : '#ddd',
                borderRadius: '4px', cursor: 'pointer',
                backgroundColor: profile === p.id ? 'var(--accent)' : 'white',
                color: profile === p.id ? 'white' : '#333',
              }}
            >
              {p.label}
            </button>
          ))}
          {isCustom && (
            <span style={{ fontSize: '11px', color: '#999', marginLeft: '4px' }}>{t('toolsTab.custom')}</span>
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

      {/* 内置工具列表 */}
      {builtinTools.length > 0 && (
        <div style={{ marginBottom: '12px' }}>
          <div style={{ fontSize: '12px', color: '#666', marginBottom: '6px' }}>
            {t('toolsTab.builtinTools')} ({builtinTools.length})
          </div>
          {builtinTools.map(tool => (
            <ToolRow key={tool.name} tool={tool} onToggle={handleToggleTool} />
          ))}
        </div>
      )}

      {/* MCP 工具列表 */}
      {mcpTools.length > 0 && (
        <div>
          <div style={{ fontSize: '12px', color: '#666', marginBottom: '6px' }}>
            {t('toolsTab.mcpTools')} ({mcpTools.length})
          </div>
          {mcpTools.map(tool => (
            <ToolRow key={tool.name} tool={tool} onToggle={handleToggleTool} />
          ))}
        </div>
      )}

      {tools.length === 0 && (
        <div style={{ textAlign: 'center', color: '#999', fontSize: '13px', padding: '20px 0' }}>
          {t('toolsTab.noTools')}
        </div>
      )}
    </div>
  )
}

/** 单个工具行组件 */
function ToolRow({ tool, onToggle }: { tool: ToolInfo; onToggle: (name: string, enabled: boolean) => void }) {
  const { t } = useI18n()
  const safetyKey = tool.safety?.toLowerCase() || 'safe'
  const colors = SAFETY_COLORS[safetyKey] || SAFETY_COLORS.safe
  const safetyLabels: Record<string, string> = {
    safe: t('toolsTab.safetySafe'),
    guarded: t('toolsTab.safetyGuarded'),
    sandboxed: t('toolsTab.safetySandboxed'),
    approval: t('toolsTab.safetyApproval'),
  }
  const label = safetyLabels[safetyKey] || tool.safety

  return (
    <div style={{
      display: 'flex', alignItems: 'center', padding: '6px 8px',
      marginBottom: '4px', borderRadius: '4px', backgroundColor: '#fafafa',
      gap: '8px',
    }}>
      {/* 工具信息 */}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: '13px', fontWeight: 500, color: '#333' }}>{tool.name}</div>
        {tool.description && (
          <div style={{
            fontSize: '11px', color: '#999', marginTop: '2px',
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
            {tool.description}
          </div>
        )}
      </div>

      {/* 安全等级标签 */}
      <span style={{
        fontSize: '10px', padding: '2px 6px', borderRadius: '3px',
        backgroundColor: colors.bg, color: colors.color, whiteSpace: 'nowrap',
      }}>
        {label}
      </span>

      {/* 开关 */}
      <label style={{ position: 'relative', display: 'inline-block', width: '32px', height: '18px', flexShrink: 0 }}>
        <input
          type="checkbox"
          checked={tool.enabled}
          onChange={e => onToggle(tool.name, e.target.checked)}
          style={{ opacity: 0, width: 0, height: 0 }}
        />
        <span style={{
          position: 'absolute', cursor: 'pointer', top: 0, left: 0, right: 0, bottom: 0,
          backgroundColor: tool.enabled ? 'var(--accent)' : '#ccc',
          borderRadius: '9px', transition: 'background-color 0.2s',
        }}>
          <span style={{
            position: 'absolute', content: '""', height: '14px', width: '14px',
            left: tool.enabled ? '16px' : '2px', bottom: '2px',
            backgroundColor: 'white', borderRadius: '50%', transition: 'left 0.2s',
          }} />
        </span>
      </label>
    </div>
  )
}
