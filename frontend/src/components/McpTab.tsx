/**
 * MCP Server 管理 Tab
 *
 * 功能：
 * - 列出 Agent 关联的 MCP Server + 状态指示器
 * - 添加新 MCP Server（stdio/HTTP）
 * - 测试连接
 * - 导入 Claude Desktop 配置
 * - 删除/启用禁用 Server
 */

import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import Select from './Select'

interface McpServer {
  id: string
  agent_id: string
  name: string
  transport: string
  command?: string
  args?: string[]
  url?: string
  env?: Record<string, string>
  enabled: boolean
  status: string
  created_at: number
}

interface McpTool {
  name: string
  description: string
}

interface McpTabProps {
  agentId: string
}

const STATUS_COLORS: Record<string, string> = {
  connected: '#22c55e',
  configured: '#eab308',
  failed: '#ef4444',
}

export default function McpTab({ agentId }: McpTabProps) {
  const { t } = useI18n()
  const [servers, setServers] = useState<McpServer[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [expandedServer, setExpandedServer] = useState<string | null>(null)
  const [serverTools, setServerTools] = useState<Record<string, McpTool[]>>({})
  const [testing, setTesting] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const [error, setError] = useState('')
  const [showJsonImport, setShowJsonImport] = useState(false)
  const [jsonInput, setJsonInput] = useState('')

  // 添加表单
  const [form, setForm] = useState({
    name: '', transport: 'stdio' as 'stdio' | 'http',
    command: '', args: '', url: '',
    envKeys: [''], envValues: [''],
  })

  const loadServers = async () => {
    try {
      const list = await invoke<McpServer[]>('list_mcp_servers', { agentId })
      setServers(list)
    } catch (e) { setError(String(e)) }
  }

  useEffect(() => { loadServers() }, [agentId])

  const handleAdd = async () => {
    try {
      setError('')
      const env: Record<string, string> = {}
      form.envKeys.forEach((k, i) => {
        if (k.trim()) env[k.trim()] = form.envValues[i] || ''
      })
      const args = form.args.trim() ? form.args.split(/\s+/) : undefined

      await invoke('add_mcp_server', {
        agentId, name: form.name, transport: form.transport,
        command: form.transport === 'stdio' ? form.command : null,
        args: form.transport === 'stdio' ? args : null,
        url: form.transport === 'http' ? form.url : null,
        env: Object.keys(env).length > 0 ? env : null,
      })
      setShowAdd(false)
      setForm({ name: '', transport: 'stdio', command: '', args: '', url: '', envKeys: [''], envValues: [''] })
      await loadServers()
    } catch (e) { setError(String(e)) }
  }

  const handleRemove = async (serverId: string) => {
    try {
      await invoke('remove_mcp_server', { serverId })
      await loadServers()
    } catch (e) { setError(String(e)) }
  }

  const handleToggle = async (serverId: string, enabled: boolean) => {
    try {
      await invoke('toggle_mcp_server', { serverId, enabled })
      await loadServers()
    } catch (e) { setError(String(e)) }
  }

  const handleTest = async (serverId: string) => {
    try {
      setTesting(serverId)
      setError('')
      const tools = await invoke<McpTool[]>('test_mcp_connection', { serverId })
      setServerTools(prev => ({ ...prev, [serverId]: tools }))
      setExpandedServer(serverId)
      await loadServers()
    } catch (e) {
      setError(String(e))
      await loadServers()
    } finally { setTesting(null) }
  }

  const handleImport = async () => {
    try {
      setImporting(true)
      setError('')
      const imported = await invoke<McpServer[]>('import_claude_mcp_config', { agentId })
      await loadServers()
      toast.success(t('mcpTab.importSuccess', { count: imported.length }))
    } catch (e) { setError(String(e)) }
    finally { setImporting(false) }
  }

  const handleJsonImport = async () => {
    if (!jsonInput.trim()) return
    try {
      setError('')
      const parsed = JSON.parse(jsonInput.trim())
      // 支持两种格式：{ mcpServers: {...} } 或直接 { name: { command, args } }
      const servers = parsed.mcpServers || parsed
      if (typeof servers !== 'object' || Array.isArray(servers)) {
        setError('JSON 格式错误：需要 { "mcpServers": { "name": { "command": "...", "args": [...] } } } 或 { "name": { "command": "..." } }')
        return
      }
      let count = 0
      for (const [name, cfg] of Object.entries(servers)) {
        const c = cfg as Record<string, unknown>
        if (!c.command) continue
        const args = Array.isArray(c.args) ? (c.args as string[]).join(' ') : ''
        const env = c.env ? JSON.stringify(c.env) : undefined
        await invoke('add_mcp_server', {
          agentId,
          name,
          transport: c.type || 'stdio',
          command: c.command,
          args: args || undefined,
          url: c.url || undefined,
          env,
        })
        count++
      }
      await loadServers()
      setJsonInput('')
      setShowJsonImport(false)
      toast.success(`已导入 ${count} 个 MCP Server`)
    } catch (e) {
      setError(`JSON 解析失败: ${e}`)
    }
  }

  return (
    <div>
      {/* 操作栏 */}
      <div style={{ display: 'flex', gap: '6px', marginBottom: '10px', flexWrap: 'wrap' }}>
        <button onClick={() => { setShowAdd(!showAdd); setShowJsonImport(false) }} style={btnStyle}>
          {showAdd ? t('mcpTab.cancelBtn') : t('mcpTab.addBtn')}
        </button>
        <button onClick={handleImport} disabled={importing} style={btnStyle}>
          {importing ? t('mcpTab.importing') : t('mcpTab.importClaude')}
        </button>
        <button onClick={() => { setShowJsonImport(!showJsonImport); setShowAdd(false) }} style={btnStyle}>
          {showJsonImport ? t('mcpTab.cancelBtn') : 'JSON'}
        </button>
      </div>

      {error && <div style={{ color: 'var(--error)', fontSize: '12px', marginBottom: '8px' }}>{error}</div>}

      {/* JSON 粘贴导入 */}
      {showJsonImport && (
        <div style={{ border: '1px solid var(--border-subtle)', borderRadius: 8, padding: 12, marginBottom: 12 }}>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginBottom: 8 }}>
            粘贴 MCP 配置 JSON（支持 Claude/Cursor 格式）：
          </div>
          <textarea
            value={jsonInput}
            onChange={e => setJsonInput(e.target.value)}
            placeholder={`{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}`}
            rows={8}
            style={{
              width: '100%', padding: 10, borderRadius: 6,
              border: '1px solid var(--border-subtle)', fontSize: 12,
              fontFamily: 'monospace', backgroundColor: 'var(--bg-glass)',
              color: 'var(--text-primary)', resize: 'vertical',
            }}
          />
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 8 }}>
            <button onClick={handleJsonImport} disabled={!jsonInput.trim()}
              style={{ padding: '6px 16px', borderRadius: 6, border: 'none', backgroundColor: 'var(--accent)', color: '#fff', fontSize: 12, cursor: 'pointer' }}>
              导入
            </button>
          </div>
        </div>
      )}

      {/* 添加表单 */}
      {showAdd && (
        <div style={{ border: '1px solid var(--border-subtle)', borderRadius: '6px', padding: '10px', marginBottom: '10px', fontSize: '12px' }}>
          <div style={{ marginBottom: '6px' }}>
            <label>{t('mcpTab.fieldName')}</label>
            <input value={form.name} onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
              style={inputStyle} placeholder="my-server" />
          </div>
          <div style={{ marginBottom: '6px' }}>
            <label>{t('mcpTab.fieldType')}</label>
            <Select value={form.transport} onChange={v => setForm(f => ({ ...f, transport: v as 'stdio' | 'http' }))}
              options={[
                { value: 'stdio', label: t('mcpTab.typeStdio') },
                { value: 'http', label: t('mcpTab.typeHttp') },
              ]}
              style={{ width: '100%' }} />
          </div>
          {form.transport === 'stdio' ? (
            <>
              <div style={{ marginBottom: '6px' }}>
                <label>{t('mcpTab.fieldCommand')}</label>
                <input value={form.command} onChange={e => setForm(f => ({ ...f, command: e.target.value }))}
                  style={inputStyle} placeholder="npx" />
              </div>
              <div style={{ marginBottom: '6px' }}>
                <label>{t('mcpTab.fieldArgs')}</label>
                <input value={form.args} onChange={e => setForm(f => ({ ...f, args: e.target.value }))}
                  style={inputStyle} placeholder="-y @modelcontextprotocol/server-filesystem /tmp" />
              </div>
            </>
          ) : (
            <div style={{ marginBottom: '6px' }}>
              <label>{t('mcpTab.fieldUrl')}</label>
              <input value={form.url} onChange={e => setForm(f => ({ ...f, url: e.target.value }))}
                style={inputStyle} placeholder="http://localhost:3001/mcp" />
            </div>
          )}
          {/* 环境变量 */}
          <div style={{ marginBottom: '6px' }}>
            <label>{t('mcpTab.fieldEnv')}</label>
            {form.envKeys.map((k, i) => (
              <div key={i} style={{ display: 'flex', gap: '4px', marginTop: '4px' }}>
                <input value={k} onChange={e => {
                  const keys = [...form.envKeys]; keys[i] = e.target.value
                  setForm(f => ({ ...f, envKeys: keys }))
                }} style={{ ...inputStyle, flex: 1 }} placeholder="KEY" />
                <input value={form.envValues[i]} onChange={e => {
                  const vals = [...form.envValues]; vals[i] = e.target.value
                  setForm(f => ({ ...f, envValues: vals }))
                }} style={{ ...inputStyle, flex: 1 }} placeholder="VALUE" />
                {i === form.envKeys.length - 1 && (
                  <button onClick={() => setForm(f => ({
                    ...f, envKeys: [...f.envKeys, ''], envValues: [...f.envValues, '']
                  }))} style={{ ...btnStyle, padding: '2px 6px' }}>+</button>
                )}
              </div>
            ))}
          </div>
          <button onClick={handleAdd} disabled={!form.name.trim()}
            style={{ ...btnStyle, backgroundColor: 'var(--accent)', color: '#fff', width: '100%' }}>
            {t('mcpTab.submitAdd')}
          </button>
        </div>
      )}

      {/* Server 列表 */}
      {servers.length === 0 && !showAdd && (
        <div style={{ color: 'var(--text-muted)', fontSize: '12px', textAlign: 'center', padding: '20px 0' }}>
          {t('mcpTab.emptyServers')}
        </div>
      )}

      {servers.map(s => (
        <div key={s.id} style={{
          border: '1px solid var(--border-subtle)', borderRadius: '6px', padding: '8px',
          marginBottom: '6px', fontSize: '12px',
        }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
              <span style={{
                display: 'inline-block', width: 8, height: 8, borderRadius: '50%',
                backgroundColor: STATUS_COLORS[s.status] || '#9ca3af',
              }} />
              <span style={{ fontWeight: 600 }}>{s.name}</span>
              <span style={{ color: 'var(--text-muted)', fontSize: '11px' }}>{s.transport}</span>
            </div>
            <div style={{ display: 'flex', gap: '4px' }}>
              <input type="checkbox" checked={s.enabled}
                onChange={e => handleToggle(s.id, e.target.checked)} title={t('mcpTab.enableDisable')} />
              <button onClick={() => handleTest(s.id)} disabled={testing === s.id}
                style={{ ...btnStyle, padding: '1px 6px', fontSize: '11px' }}>
                {testing === s.id ? t('mcpTab.testing') : t('mcpTab.testBtn')}
              </button>
              <button onClick={() => handleRemove(s.id)}
                style={{ ...btnStyle, padding: '1px 6px', fontSize: '11px', color: 'var(--error)' }}>
                ✕
              </button>
            </div>
          </div>

          {/* 展开工具列表 */}
          {expandedServer === s.id && serverTools[s.id] && (
            <div style={{ marginTop: '6px', paddingTop: '6px', borderTop: '1px solid var(--border-subtle)' }}>
              <div style={{ color: 'var(--text-secondary)', marginBottom: '4px' }}>
                {t('mcpTab.tools')} ({serverTools[s.id].length}):
              </div>
              {serverTools[s.id].map(t => (
                <div key={t.name} style={{ padding: '2px 0', color: 'var(--text-primary)' }}>
                  <span style={{ fontWeight: 500 }}>{t.name}</span>
                  <span style={{ color: 'var(--text-muted)', marginLeft: '6px' }}>{t.description}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  )
}

const btnStyle: React.CSSProperties = {
  padding: '4px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px',
  backgroundColor: 'var(--bg-elevated)', cursor: 'pointer', fontSize: '12px',
}

const inputStyle: React.CSSProperties = {
  width: '100%', padding: '4px 8px', border: '1px solid var(--border-subtle)',
  borderRadius: '4px', fontSize: '12px', boxSizing: 'border-box',
}
