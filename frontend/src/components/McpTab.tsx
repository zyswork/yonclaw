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

const STATUS_ICONS: Record<string, string> = {
  connected: '🟢',
  configured: '🟡',
  failed: '🔴',
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
      alert(t('mcpTab.importSuccess', { count: imported.length }))
    } catch (e) { setError(String(e)) }
    finally { setImporting(false) }
  }

  return (
    <div>
      {/* 操作栏 */}
      <div style={{ display: 'flex', gap: '6px', marginBottom: '10px' }}>
        <button onClick={() => setShowAdd(!showAdd)} style={btnStyle}>
          {showAdd ? t('mcpTab.cancelBtn') : t('mcpTab.addBtn')}
        </button>
        <button onClick={handleImport} disabled={importing} style={btnStyle}>
          {importing ? t('mcpTab.importing') : t('mcpTab.importClaude')}
        </button>
      </div>

      {error && <div style={{ color: 'red', fontSize: '12px', marginBottom: '8px' }}>{error}</div>}

      {/* 添加表单 */}
      {showAdd && (
        <div style={{ border: '1px solid #ddd', borderRadius: '6px', padding: '10px', marginBottom: '10px', fontSize: '12px' }}>
          <div style={{ marginBottom: '6px' }}>
            <label>{t('mcpTab.fieldName')}</label>
            <input value={form.name} onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
              style={inputStyle} placeholder="my-server" />
          </div>
          <div style={{ marginBottom: '6px' }}>
            <label>{t('mcpTab.fieldType')}</label>
            <select value={form.transport} onChange={e => setForm(f => ({ ...f, transport: e.target.value as 'stdio' | 'http' }))}
              style={inputStyle}>
              <option value="stdio">{t('mcpTab.typeStdio')}</option>
              <option value="http">{t('mcpTab.typeHttp')}</option>
            </select>
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
        <div style={{ color: '#999', fontSize: '12px', textAlign: 'center', padding: '20px 0' }}>
          {t('mcpTab.emptyServers')}
        </div>
      )}

      {servers.map(s => (
        <div key={s.id} style={{
          border: '1px solid #eee', borderRadius: '6px', padding: '8px',
          marginBottom: '6px', fontSize: '12px',
        }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
              <span>{STATUS_ICONS[s.status] || '⚪'}</span>
              <span style={{ fontWeight: 600 }}>{s.name}</span>
              <span style={{ color: '#999', fontSize: '11px' }}>{s.transport}</span>
            </div>
            <div style={{ display: 'flex', gap: '4px' }}>
              <input type="checkbox" checked={s.enabled}
                onChange={e => handleToggle(s.id, e.target.checked)} title={t('mcpTab.enableDisable')} />
              <button onClick={() => handleTest(s.id)} disabled={testing === s.id}
                style={{ ...btnStyle, padding: '1px 6px', fontSize: '11px' }}>
                {testing === s.id ? t('mcpTab.testing') : t('mcpTab.testBtn')}
              </button>
              <button onClick={() => handleRemove(s.id)}
                style={{ ...btnStyle, padding: '1px 6px', fontSize: '11px', color: 'red' }}>
                ✕
              </button>
            </div>
          </div>

          {/* 展开工具列表 */}
          {expandedServer === s.id && serverTools[s.id] && (
            <div style={{ marginTop: '6px', paddingTop: '6px', borderTop: '1px solid #eee' }}>
              <div style={{ color: '#666', marginBottom: '4px' }}>
                {t('mcpTab.tools')} ({serverTools[s.id].length}):
              </div>
              {serverTools[s.id].map(t => (
                <div key={t.name} style={{ padding: '2px 0', color: '#444' }}>
                  <span style={{ fontWeight: 500 }}>{t.name}</span>
                  <span style={{ color: '#999', marginLeft: '6px' }}>{t.description}</span>
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
  padding: '4px 10px', border: '1px solid #ddd', borderRadius: '4px',
  backgroundColor: '#fff', cursor: 'pointer', fontSize: '12px',
}

const inputStyle: React.CSSProperties = {
  width: '100%', padding: '4px 8px', border: '1px solid #ddd',
  borderRadius: '4px', fontSize: '12px', boxSizing: 'border-box',
}
