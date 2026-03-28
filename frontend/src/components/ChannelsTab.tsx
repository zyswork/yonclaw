/**
 * Agent 频道配置 Tab — 每个 Agent 独立配置自己的 bot/app
 */
import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import Modal from './Modal'

interface AgentChannel {
  id: string
  agentId: string
  channelType: string
  credentials: Record<string, string>
  displayName: string | null
  enabled: boolean
  status: string
  statusMessage: string | null
  createdAt: number
}

const CHANNEL_TYPES = [
  { type: 'telegram', name: 'Telegram', icon: 'TG', fields: [{ key: 'bot_token', label: 'Bot Token', placeholder: '123456:ABC-DEF...', secret: true }] },
  { type: 'feishu', name: 'Feishu / Lark', icon: 'FS', fields: [{ key: 'app_id', label: 'App ID', placeholder: 'cli_xxx', secret: false }, { key: 'app_secret', label: 'App Secret', placeholder: '', secret: true }] },
  { type: 'discord', name: 'Discord', icon: 'DC', fields: [{ key: 'bot_token', label: 'Bot Token', placeholder: 'MTIz...NzY', secret: true }] },
  { type: 'slack', name: 'Slack', icon: 'SK', fields: [{ key: 'bot_token', label: 'Bot Token (xoxb-)', placeholder: 'xoxb-...', secret: true }, { key: 'app_token', label: 'App Token (xapp-)', placeholder: 'xapp-...', secret: true }] },
  { type: 'weixin', name: 'WeChat', icon: 'WX', fields: [{ key: 'bot_token', label: 'iLinkai Token', placeholder: '', secret: true }] },
]

const STATUS_COLORS: Record<string, string> = {
  running: '#22c55e',
  configured: '#f0ad4e',
  stopped: '#9ca3af',
  error: '#ef4444',
}

export default function ChannelsTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [channels, setChannels] = useState<AgentChannel[]>([])
  const [adding, setAdding] = useState<string | null>(null)
  const [formValues, setFormValues] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)

  const load = useCallback(async () => {
    try {
      const result = await invoke<AgentChannel[]>('list_agent_channels', { agentId })
      setChannels(result || [])
    } catch (e) { console.error(e) }
  }, [agentId])

  useEffect(() => { load() }, [load])

  const handleAdd = async (channelType: string) => {
    setSaving(true)
    try {
      await invoke('create_agent_channel', {
        agentId,
        channelType,
        credentials: formValues,
        displayName: null,
      })
      toast.success(t('agentChannels.created'))
      setAdding(null)
      setFormValues({})
      await load()
    } catch (e) { toast.error(String(e)) }
    setSaving(false)
  }

  const handleDelete = async (id: string) => {
    try {
      await invoke('delete_agent_channel', { id })
      toast.success(t('agentChannels.deleted'))
      await load()
    } catch (e) { toast.error(String(e)) }
  }

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      await invoke('toggle_agent_channel', { id, enabled })
      await load()
    } catch (e) { toast.error(String(e)) }
  }

  // 已配置的频道类型
  const configuredTypes = new Set(channels.map(c => c.channelType))

  return (
    <div style={{ padding: 20, maxWidth: 800 }}>
      <h3 style={{ margin: '0 0 8px', fontSize: 16, fontWeight: 600 }}>{t('agentChannels.title')}</h3>
      <p style={{ fontSize: 13, color: 'var(--text-muted)', margin: '0 0 20px' }}>
        {t('agentChannels.desc')}
      </p>

      {/* 已配置的频道 */}
      {channels.length > 0 && (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 20 }}>
          {channels.map(ch => {
            const def = CHANNEL_TYPES.find(d => d.type === ch.channelType)
            return (
              <div key={ch.id} style={{
                padding: '14px 18px', borderRadius: 10,
                border: `1px solid ${ch.status === 'running' ? 'var(--success)' : 'var(--border-subtle)'}`,
                backgroundColor: 'var(--bg-elevated)',
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 20 }}>{def?.icon || '--'}</span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 14, fontWeight: 600 }}>
                      {def?.name || ch.channelType}
                      {ch.displayName && <span style={{ fontWeight: 400, color: 'var(--text-muted)', marginLeft: 6 }}>({ch.displayName})</span>}
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-muted)', display: 'flex', alignItems: 'center', gap: 6 }}>
                      <span style={{
                        width: 6, height: 6, borderRadius: '50%',
                        backgroundColor: STATUS_COLORS[ch.status] || '#9ca3af',
                      }} />
                      {ch.status}
                      {ch.statusMessage && <span>· {ch.statusMessage}</span>}
                    </div>
                  </div>
                  {/* 凭证预览 */}
                  <div style={{ fontSize: 11, color: 'var(--text-muted)', textAlign: 'right' }}>
                    {Object.entries(ch.credentials || {}).map(([k, v]) => (
                      <div key={k}>{k}: {v}</div>
                    ))}
                  </div>
                  {/* 操作按钮 */}
                  <div style={{ display: 'flex', gap: 6 }}>
                    <button onClick={() => handleToggle(ch.id, !ch.enabled)}
                      style={{
                        padding: '4px 10px', fontSize: 11, borderRadius: 4,
                        border: '1px solid var(--border-subtle)', cursor: 'pointer',
                        backgroundColor: ch.enabled ? 'var(--success-bg)' : 'transparent',
                        color: ch.enabled ? 'var(--success)' : 'var(--text-muted)',
                      }}>
                      {ch.enabled ? t('agentChannels.enabled') : t('agentChannels.disabled')}
                    </button>
                    <button onClick={() => handleDelete(ch.id)}
                      style={{
                        padding: '4px 10px', fontSize: 11, borderRadius: 4,
                        border: '1px solid var(--border-subtle)', cursor: 'pointer',
                        color: 'var(--error)',
                      }}>
                      {t('common.delete')}
                    </button>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* 添加新频道 */}
      <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 10 }}>
        {t('agentChannels.addNew')}
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 8 }}>
        {CHANNEL_TYPES.filter(d => !configuredTypes.has(d.type)).map(def => (
          <button key={def.type}
            onClick={() => { setAdding(def.type); setFormValues({}) }}
            style={{
              padding: '12px', borderRadius: 8, border: '1px solid var(--border-subtle)',
              backgroundColor: 'var(--bg-glass)', cursor: 'pointer',
              display: 'flex', alignItems: 'center', gap: 8, fontSize: 13,
            }}
          >
            <span style={{ fontSize: 18 }}>{def.icon}</span>
            {def.name}
          </button>
        ))}
        {CHANNEL_TYPES.filter(d => !configuredTypes.has(d.type)).length === 0 && (
          <div style={{ gridColumn: '1 / -1', color: 'var(--text-muted)', fontSize: 12, padding: 8 }}>
            {t('agentChannels.allConfigured')}
          </div>
        )}
      </div>

      {/* 添加表单弹窗 */}
      {(() => {
        const def = adding ? CHANNEL_TYPES.find(d => d.type === adding) : null
        return (
          <Modal open={!!adding && !!def} onClose={() => setAdding(null)} width={400}
            title={def ? `${def.icon} ${t('agentChannels.configure')} ${def.name}` : ''}
            footer={
              <>
                <button onClick={() => setAdding(null)}
                  style={{ padding: '8px 16px', borderRadius: 6, border: '1px solid var(--border-subtle)', cursor: 'pointer', fontSize: 13, color: 'var(--text-secondary)' }}>
                  {t('common.cancel')}
                </button>
                <button onClick={() => adding && handleAdd(adding)} disabled={saving}
                  style={{
                    padding: '8px 20px', borderRadius: 6, border: 'none',
                    backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer', fontSize: 13,
                  }}>
                  {saving ? t('common.saving') : t('agentChannels.connect')}
                </button>
              </>
            }
          >
            {def?.fields.map(f => (
              <div key={f.key} style={{ marginBottom: 12 }}>
                <label style={{ fontSize: 12, fontWeight: 500, display: 'block', marginBottom: 4, color: 'var(--text-secondary)' }}>
                  {f.label}
                </label>
                <input
                  type={f.secret ? 'password' : 'text'}
                  value={formValues[f.key] || ''}
                  onChange={e => setFormValues(prev => ({ ...prev, [f.key]: e.target.value }))}
                  placeholder={f.placeholder}
                  style={{
                    width: '100%', padding: '8px 12px', borderRadius: 6, fontSize: 13,
                    border: '1px solid var(--border-subtle)', boxSizing: 'border-box',
                    backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
                  }}
                />
              </div>
            ))}
          </Modal>
        )
      })()}
    </div>
  )
}
