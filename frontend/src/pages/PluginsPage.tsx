/**
 * 插件管理 — 系统级插件（渠道/模型提供商/记忆后端/功能扩展）
 * 支持全局启停 + per-Agent 配置
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface PluginInfo {
  id: string; name: string; version: string; description: string
  pluginType: string; builtin: boolean; icon: string
  enabled: boolean; defaultEnabled: boolean; status: string; connected?: boolean
  configSchema: { key: string; label: string; field_type: string; required: boolean; default?: string; placeholder?: string }[]
}

interface AgentPluginState {
  pluginId: string; enabled: boolean; configOverride?: string
}

const TYPE_ORDER = ['模型提供商', '渠道', '记忆后端', '嵌入模型', '功能扩展']
// TYPE_LABELS keys match backend pluginType values; display labels use i18n
const TYPE_LABEL_KEYS: Record<string, string> = {
  '模型提供商': 'plugins.typeModel', '渠道': 'plugins.typeChannel', '记忆后端': 'plugins.typeMemory', '嵌入模型': 'plugins.typeEmbedding', '功能扩展': 'plugins.typeFeatures',
}

export default function PluginsPage() {
  const { t } = useI18n()
  const [plugins, setPlugins] = useState<PluginInfo[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [agentStates, setAgentStates] = useState<AgentPluginState[]>([])
  const [activeType, setActiveType] = useState('all')
  const [configuring, setConfiguring] = useState<string | null>(null)
  const [configValues, setConfigValues] = useState<Record<string, string>>({})
  const [loading, setLoading] = useState(true)

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { loadPlugins() }, [])
  useEffect(() => { if (selectedAgent) loadAgentStates() }, [selectedAgent])

  const loadAgents = async () => {
    try {
      const list = (await invoke('list_agents')) as any[]
      setAgents(list.map((a: any) => ({ id: a.id, name: a.name })))
      if (list.length > 0) setSelectedAgent(list[0].id)
    } catch {}
    setLoading(false)
  }

  const loadPlugins = async () => {
    try {
      const list = (await invoke('list_system_plugins')) as PluginInfo[]
      setPlugins(list)
    } catch (e) { console.error(e) }
  }

  const loadAgentStates = async () => {
    try {
      const states = (await invoke('get_agent_plugin_states', { agentId: selectedAgent })) as AgentPluginState[]
      setAgentStates(states)
    } catch {}
  }

  const togglePlugin = async (pluginId: string, currentEnabled: boolean) => {
    try {
      await invoke('toggle_system_plugin', { pluginId, enabled: !currentEnabled })
      await loadPlugins()
    } catch (e) { alert(t('cronExtra.operationFailed') + ': ' + e) }
  }

  const toggleAgentPlugin = async (pluginId: string, currentEnabled: boolean) => {
    try {
      await invoke('set_agent_plugin', { agentId: selectedAgent, pluginId, enabled: !currentEnabled })
      await loadAgentStates()
    } catch (e) { alert(t('cronExtra.operationFailed') + ': ' + e) }
  }

  const openConfig = async (pluginId: string) => {
    try {
      const json = await invoke<string>('get_plugin_config', { pluginId })
      setConfigValues(JSON.parse(json || '{}'))
    } catch { setConfigValues({}) }
    setConfiguring(pluginId)
  }

  const saveConfig = async () => {
    if (!configuring) return
    try {
      await invoke('save_plugin_config', { pluginId: configuring, configJson: JSON.stringify(configValues) })
      setConfiguring(null)
      setConfigValues({})
    } catch (e) { alert(t('settingsExtra.saveFailed') + ': ' + e) }
  }

  const getAgentEnabled = (pluginId: string, globalEnabled: boolean): boolean => {
    const state = agentStates.find(s => s.pluginId === pluginId)
    return state ? state.enabled : globalEnabled
  }

  const types = ['all', ...TYPE_ORDER]
  const filtered = activeType === 'all' ? plugins : plugins.filter(p => p.pluginType === activeType)

  // 分组
  const grouped: Record<string, PluginInfo[]> = {}
  for (const p of filtered) {
    if (!grouped[p.pluginType]) grouped[p.pluginType] = []
    grouped[p.pluginType].push(p)
  }

  if (loading) return <div style={{ padding: 24, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  const configuringPlugin = plugins.find(p => p.id === configuring)

  return (
    <div style={{ padding: '24px 32px', maxWidth: 900 }}>
      {/* 标题 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 6 }}>
        <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>{t('plugins.title')}</h1>
        <span style={{ fontSize: 12, padding: '2px 8px', borderRadius: 10, backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)' }}>
          {plugins.length}{t('plugins.labelCount')}
        </span>
        <span style={{ flex: 1 }} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{t('plugins.labelAgent')}</span>
          <select value={selectedAgent} onChange={e => setSelectedAgent(e.target.value)}
            style={{ padding: '5px 10px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 12 }}>
            {agents.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
        </div>
      </div>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 16px' }}>
        {t('plugins.hintSwitches')}
      </p>

      {/* 分类 Tab */}
      <div style={{ display: 'flex', gap: 4, marginBottom: 20, flexWrap: 'wrap' }}>
        {types.map(tp => (
          <button key={tp} onClick={() => setActiveType(tp)}
            style={{
              padding: '5px 12px', borderRadius: 14, fontSize: 12, cursor: 'pointer',
              backgroundColor: activeType === tp ? 'var(--accent)' : 'var(--bg-glass)',
              color: activeType === tp ? '#fff' : 'var(--text-secondary)',
              border: 'none', fontWeight: activeType === tp ? 600 : 400,
            }}>
            {tp === 'all' ? t('plugins.tabAll') : (TYPE_LABEL_KEYS[tp] ? t(TYPE_LABEL_KEYS[tp]) : tp)}
          </button>
        ))}
      </div>

      {/* 配置弹窗 */}
      {configuring && configuringPlugin && (
        <div style={{
          marginBottom: 20, padding: 16, borderRadius: 10,
          backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--accent)',
        }}>
          <div style={{ fontWeight: 600, fontSize: 14, marginBottom: 12 }}>
            {configuringPlugin.icon} {configuringPlugin.name} {t('plugins.btnConfig')}
          </div>
          {configuringPlugin.configSchema.map(field => (
            <div key={field.key} style={{ marginBottom: 10 }}>
              <label style={{ fontSize: 12, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>
                {field.label} {field.required && <span style={{ color: 'var(--error)' }}>*</span>}
              </label>
              <input
                type={field.field_type === 'password' ? 'password' : 'text'}
                value={configValues[field.key] || ''}
                placeholder={field.placeholder || field.default || ''}
                onChange={e => setConfigValues({ ...configValues, [field.key]: e.target.value })}
                style={{
                  width: '100%', padding: '7px 10px', borderRadius: 6, fontSize: 13,
                  border: '1px solid var(--border-subtle)', boxSizing: 'border-box',
                }}
              />
            </div>
          ))}
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <button onClick={saveConfig}
              style={{ padding: '6px 16px', borderRadius: 6, backgroundColor: 'var(--accent)', color: '#fff', border: 'none', fontSize: 12, cursor: 'pointer' }}>
              {t('common.save')}
            </button>
            <button onClick={() => { setConfiguring(null); setConfigValues({}) }}
              style={{ padding: '6px 16px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 12, cursor: 'pointer' }}>
              {t('common.cancel')}
            </button>
          </div>
        </div>
      )}

      {/* 插件列表 */}
      {(activeType === 'all' ? TYPE_ORDER : [activeType]).map(type => {
        const items = grouped[type]
        if (!items || items.length === 0) return null
        return (
          <div key={type} style={{ marginBottom: 24 }}>
            <h3 style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 10, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
              {type}
            </h3>
            {items.map(plugin => {
              const agentEnabled = getAgentEnabled(plugin.id, plugin.enabled)
              return (
                <div key={plugin.id} style={{
                  display: 'flex', alignItems: 'center', gap: 12, padding: '12px 14px',
                  borderRadius: 10, marginBottom: 6,
                  backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
                }}>
                  {/* 图标 */}
                  <div style={{
                    width: 36, height: 36, borderRadius: 8, backgroundColor: 'var(--bg-glass)',
                    display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 18, flexShrink: 0,
                  }}>
                    {plugin.icon || '\u{1F9E9}'}
                  </div>

                  {/* 信息 */}
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                      <span style={{ fontSize: 13, fontWeight: 600 }}>{plugin.name}</span>
                      {plugin.builtin && plugin.status === 'active' && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 3, backgroundColor: '#6366F1', color: '#fff' }}>{t('plugins.labelBuiltin')}</span>}
                      {plugin.status === 'ready' && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 3, backgroundColor: '#f59e0b', color: '#fff' }}>{t('plugins.statusReady')}</span>}
                      {plugin.status === 'planned' && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 3, backgroundColor: '#9ca3af', color: '#fff' }}>{t('plugins.statusPlanned')}</span>}
                      {plugin.connected && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 3, backgroundColor: '#22c55e', color: '#fff' }}>{t('plugins.statusConnected')}</span>}
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {plugin.description}
                    </div>
                  </div>

                  {/* 配置按钮 */}
                  {plugin.configSchema.length > 0 && plugin.status !== 'planned' && (
                    <button onClick={() => openConfig(plugin.id)}
                      style={{ padding: '4px 8px', borderRadius: 4, border: '1px solid var(--border-subtle)', fontSize: 11, cursor: 'pointer', color: 'var(--text-secondary)', backgroundColor: 'transparent' }}>
                      {t('plugins.btnConfig')}
                    </button>
                  )}

                  {/* Agent 开关 */}
                  {plugin.status === 'planned' ? (
                    <span style={{ fontSize: 11, color: 'var(--text-muted)', flexShrink: 0 }}>{t('plugins.labelComingSoon')}</span>
                  ) : (<>
                  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2, flexShrink: 0 }}>
                    <div
                      onClick={() => toggleAgentPlugin(plugin.id, agentEnabled)}
                      style={{
                        width: 34, height: 18, borderRadius: 9, cursor: 'pointer',
                        backgroundColor: agentEnabled ? 'var(--success)' : '#ccc',
                        position: 'relative', transition: 'background-color 0.2s',
                      }}>
                      <div style={{
                        width: 14, height: 14, borderRadius: '50%', backgroundColor: '#fff',
                        position: 'absolute', top: 2,
                        left: agentEnabled ? 18 : 2, transition: 'left 0.2s',
                      }} />
                    </div>
                    <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>{t('plugins.labelAgentSwitch')}</span>
                  </div>

                  {/* 全局开关 */}
                  <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2, flexShrink: 0 }}>
                    <div
                      onClick={() => togglePlugin(plugin.id, plugin.enabled)}
                      style={{
                        width: 34, height: 18, borderRadius: 9, cursor: 'pointer',
                        backgroundColor: plugin.enabled ? 'var(--accent)' : '#ccc',
                        position: 'relative', transition: 'background-color 0.2s',
                      }}>
                      <div style={{
                        width: 14, height: 14, borderRadius: '50%', backgroundColor: '#fff',
                        position: 'absolute', top: 2,
                        left: plugin.enabled ? 18 : 2, transition: 'left 0.2s',
                      }} />
                    </div>
                    <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>{t('plugins.labelGlobal')}</span>
                  </div>
                  </>)}
                </div>
              )
            })}
          </div>
        )
      })}

      {filtered.length === 0 && (
        <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('plugins.emptyType')}</div>
      )}

      <div style={{ marginTop: 20, padding: '10px 0', borderTop: '1px solid var(--border-subtle)', fontSize: 11, color: 'var(--text-muted)' }}>
        {t('plugins.hintDetail')}
      </div>
    </div>
  )
}
