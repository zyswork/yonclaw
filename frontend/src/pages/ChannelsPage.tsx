/**
 * 频道管理 — 通过 cloud_api_proxy Tauri 命令调用云端 API
 */
import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { QRCodeSVG } from 'qrcode.react'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import { useConfirm } from '../hooks/useConfirm'
import Modal from '../components/Modal'
import Select from '../components/Select'

async function cloudApi(method: string, path: string, body?: any): Promise<any> {
  return invoke('cloud_api_proxy', { method, path, body: body || null })
}

interface ChannelInfo {
  id: string; name: string; desc: string; icon: string; connected: boolean
  bot?: { username: string; name: string }
  configFields?: { key: string; label: string; placeholder: string; type?: string }[]
}

const CHANNEL_DEFS: Omit<ChannelInfo, 'desc'>[] = [
  { id: 'telegram', name: 'Telegram', icon: 'TG', connected: false, configFields: [{ key: 'botToken', label: 'Bot Token', placeholder: '123456:ABC-DEF...', type: 'password' }] },
  { id: 'feishu', name: 'Feishu / Lark', icon: 'FS', connected: false, configFields: [{ key: 'appId', label: 'App ID', placeholder: 'cli_xxx' }, { key: 'appSecret', label: 'App Secret', placeholder: '', type: 'password' }] },
  { id: 'dingtalk', name: 'DingTalk', icon: 'DD', connected: false },
  { id: 'wechat', name: 'WeChat', icon: 'WX', connected: false },
  { id: 'discord', name: 'Discord', icon: 'DC', connected: false, configFields: [{ key: 'botToken', label: 'Bot Token', placeholder: 'MTIz...NzY.Gw...', type: 'password' }] },
  { id: 'slack', name: 'Slack', icon: 'SK', connected: false, configFields: [{ key: 'botToken', label: 'Bot Token (xoxb-)', placeholder: 'xoxb-...', type: 'password' }, { key: 'appToken', label: 'App Token (xapp-)', placeholder: 'xapp-...', type: 'password' }] },
]

const DESC_KEYS: Record<string, string> = {
  telegram: 'channels.descTelegram',
  feishu: 'channels.descFeishu',
  dingtalk: 'channels.descDingtalk',
  wechat: 'channels.descWechat',
  discord: 'channels.descDiscord',
  slack: 'channels.descSlack',
}

export default function ChannelsPage() {
  const { t } = useI18n()
  const confirm = useConfirm()
  const buildChannels = (): ChannelInfo[] => CHANNEL_DEFS.map(ch => ({ ...ch, desc: t(DESC_KEYS[ch.id] || '') }))
  const [channels, setChannels] = useState<ChannelInfo[]>(() => buildChannels())
  const [configuring, setConfiguring] = useState<string | null>(null)
  const [formValues, setFormValues] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [loadError, setLoadError] = useState('')
  const [weixinQr, setWeixinQr] = useState('')
  const [allAgents, setAllAgents] = useState<{ id: string; name: string; model: string }[]>([])
  const [agentChannels, setAgentChannels] = useState<any[]>([])
  const [addingChannel, setAddingChannel] = useState<{ channelType: string } | null>(null)
  const [addAgentId, setAddAgentId] = useState('')
  const [addFormValues, setAddFormValues] = useState<Record<string, string>>({})

  const loadAgentChannels = async () => {
    try {
      const [agents, channels] = await Promise.all([
        invoke<any[]>('list_agents'),
        invoke<any[]>('list_agent_channels', { agentId: null }),
      ])
      setAllAgents(agents || [])
      setAgentChannels(channels || [])
    } catch (e) { console.error('loadAgentChannels failed:', e) }
  }

  useEffect(() => { checkStatuses(); loadAgentChannels() }, [])

  const checkStatuses = async () => {
    setLoadError('')
    try {
      // 优先从本地 settings 检查
      const token = await invoke<string | null>('get_setting', { key: 'telegram_bot_token' })
      const infoStr = await invoke<string | null>('get_setting', { key: 'telegram_bot_info' })
      const feishuId = await invoke<string | null>('get_setting', { key: 'feishu_app_id' })
      const hasFeishu = !!feishuId
      const weixinToken = await invoke<string | null>('get_setting', { key: 'weixin_bot_token' })
      const hasWeixin = !!weixinToken
      const discordToken = await invoke<string | null>('get_setting', { key: 'discord_bot_token' })
      const hasDiscord = !!discordToken
      const slackBotToken = await invoke<string | null>('get_setting', { key: 'slack_bot_token' })
      const slackAppToken = await invoke<string | null>('get_setting', { key: 'slack_app_token' })
      const hasSlack = !!slackBotToken && !!slackAppToken

      const hasToken = !!token
      let botInfo = null
      try { if (infoStr) botInfo = JSON.parse(infoStr) } catch {}

      setChannels(prev => prev.map(c =>
        c.id === 'telegram' ? { ...c, connected: hasToken, bot: botInfo }
        : c.id === 'feishu' ? { ...c, connected: hasFeishu }
        : c.id === 'wechat' ? { ...c, connected: hasWeixin }
        : c.id === 'discord' ? { ...c, connected: hasDiscord }
        : c.id === 'slack' ? { ...c, connected: hasSlack }
        : c
      ))
    } catch (e: unknown) {
      setLoadError(String(e))
    }
  }

  const handleSetup = async (channelId: string) => {
    if (channelId === 'wechat') {
      setConfiguring('wechat')
      setSaving(true); setError(t('channels.statusGettingQr')); setWeixinQr('')
      try {
        const qrData = await invoke<any>('weixin_get_qrcode')
        console.log('weixin qr data:', qrData)
        const qrcodeImg = qrData.qrcode_img_content || ''
        const qrcodeId = qrData.qrcode || ''
        if (!qrcodeId) { setError(t('channels.errorGetQr') + ': ' + JSON.stringify(qrData)); setSaving(false); return }

        console.log('weixin QR: qrcode=' + qrcodeId + ', img=' + qrcodeImg.substring(0, 80))

        // qrcode_img_content 是微信扫码链接，用 QRCodeSVG 渲染
        setWeixinQr(qrcodeImg || `https://ilinkai.weixin.qq.com/ilink/bot/get_bot_qrcode_img?qrcode=${qrcodeId}`)
        setError(t('channels.statusScanning'))

        // 轮询扫码状态（长轮询，每次请求可能 hold 30+ 秒）
        for (let i = 0; i < 10; i++) {
          try {
            const status = await invoke<any>('weixin_poll_status', { qrcode: qrcodeId })
            console.log('weixin poll result:', JSON.stringify(status))

            if (status === 'timeout' || status?.status === 'wait' || !status?.status) {
              // 长轮询超时或等待中，继续轮询
              continue
            }
            if (status.status === 'scaned') {
              setError(t('channels.statusScanned'))
              continue
            }
            if (status.status === 'confirmed') {
              const token = status.bot_token || ''
              const baseUrl = status.baseurl || ''
              if (token) {
                await invoke('weixin_save_token', { botToken: token })
                await invoke('set_setting', { key: 'weixin_bot_token', value: token })
                if (baseUrl) await invoke('set_setting', { key: 'weixin_base_url', value: baseUrl })
                setConfiguring(null); setError(''); setWeixinQr('')
                toast.success(t('channels.successConnected'))
                checkStatuses()
                setSaving(false)
                return
              } else {
                setError(t('channels.errorNoToken'))
                break
              }
            }
            if (status.status === 'expired') {
              setError(t('channels.errorQrExpired'))
              break
            }
          } catch (pe) {
            console.error('weixin poll error:', pe)
            // 超时错误继续轮询
            if (String(pe).includes('timeout')) continue
          }
        }
        setError(t('channels.errorTimeout'))
      } catch (e: unknown) { setError(t('channels.errorLoginFailed') + ': ' + String(e)); console.error(e) }
      setSaving(false)
      return
    }
    if (channelId === 'feishu') {
      const appId = formValues.appId?.trim()
      const appSecret = formValues.appSecret?.trim()
      if (!appId || !appSecret) { setError(t('channels.errorFillFields')); return }
      setSaving(true); setError('')
      try {
        await invoke('set_setting', { key: 'feishu_app_id', value: appId })
        await invoke('set_setting', { key: 'feishu_app_secret', value: appSecret })
        setConfiguring(null); setFormValues({}); setError('')
        toast.success(t('channels.successConfigured'))
        checkStatuses()
      } catch (e: unknown) { setError(t('channels.errorSaveFailed') + ': ' + String(e)) }
      setSaving(false)
      return
    }
    if (channelId === 'discord') {
      const botToken = formValues.botToken?.trim()
      if (!botToken) { setError(t('channels.errorFillToken')); return }
      setSaving(true); setError(t('channels.verifyingToken'))
      try {
        const result = await invoke<{ ok: boolean; username?: string; error?: string }>('discord_connect', { botToken })
        if (!result.ok) { setError(t('channels.tokenInvalid') + ': ' + (result.error || '')); setSaving(false); return }
        setConfiguring(null); setFormValues({}); setError('')
        toast.success(t('channels.successConnected') + ` (@${result.username})`)
        checkStatuses()
      } catch (e: unknown) { setError(t('channels.errorConnectFailed') + ': ' + String(e)) }
      setSaving(false)
      return
    }
    if (channelId === 'slack') {
      const botToken = formValues.botToken?.trim()
      const appToken = formValues.appToken?.trim()
      if (!botToken || !appToken) { setError(t('channels.errorFillFields')); return }
      setSaving(true); setError(t('channels.verifyingToken'))
      try {
        const result = await invoke<{ ok: boolean; team?: string; user?: string; error?: string }>('slack_connect', { botToken, appToken })
        if (!result.ok) { setError(t('channels.tokenInvalid') + ': ' + (result.error || '')); setSaving(false); return }
        setConfiguring(null); setFormValues({}); setError('')
        toast.success(t('channels.successConnected') + ` (${result.team})`)
        checkStatuses()
      } catch (e: unknown) { setError(t('channels.errorConnectFailed') + ': ' + String(e)) }
      setSaving(false)
      return
    }
    if (channelId !== 'telegram') { toast.info(t('channels.errorNotSupported')); return }
    const token = formValues.botToken?.trim()
    if (!token) { setError(t('channels.errorFillToken')); return }

    setSaving(true); setError('')
    try {
      // 步骤 1: 通过 Tauri Rust 侧验证 Token（绕过 WebView 限制）
      setError(t('channels.verifyingToken'))
      const verifyResult = await invoke<any>('verify_telegram_token', { botToken: token })

      if (!verifyResult.ok) { setError(t('channels.tokenInvalid') + ': ' + (verifyResult.error || '')); setSaving(false); return }
      setError(t('channels.verifySuccess', { username: verifyResult.username }))

      // 步骤 2: 保存到本地 settings（桌面端直接轮询 Telegram）
      await invoke('set_setting', { key: 'telegram_bot_token', value: token })
      await invoke('set_setting', { key: 'telegram_bot_info', value: JSON.stringify({
        username: verifyResult.username, first_name: verifyResult.name, id: verifyResult.id,
      })})

      // 注：不再发到云端（避免双重轮询）
      // 桌面离线时的 Fallback 由云端 sync 机制单独处理

      setConfiguring(null); setFormValues({}); setError('')
      toast.success(t('channels.successConnected'))
      checkStatuses()
    } catch (e: unknown) { setError(t('channels.errorConnectFailed') + ': ' + String(e)) }
    setSaving(false)
  }

  const handleDisconnect = async (channelId: string) => {
    if (!await confirm(t('channels.confirmDisconnect'))) return
    try { await cloudApi('POST', `/api/v1/channels/${channelId}/disconnect`); checkStatuses() } catch (e) { setError(String(e)) }
  }

  return (
    <div style={{ padding: '24px 32px', maxWidth: 1000 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
        <div>
          <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>{t('channels.title')}</h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: 13, margin: '4px 0 0' }}>{t('channels.subtitle')}</p>
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
          <span style={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', backgroundColor: 'var(--success)', marginRight: 6 }} />
          {channels.filter(c => c.connected).length} {t('channels.connected')}
        </div>
      </div>

      {loadError && (
        <div style={{ padding: '10px 16px', borderRadius: 8, backgroundColor: 'var(--error-bg)', color: 'var(--error)', fontSize: 13, marginTop: 12 }}>
          {loadError}
          <button onClick={checkStatuses} style={{ marginLeft: 12, fontSize: 12, padding: '2px 8px', borderRadius: 4, cursor: 'pointer' }}>{t('channels.retryBtn')}</button>
        </div>
      )}

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginTop: 20 }}>
        {channels.map(ch => (
          <div key={ch.id} style={{ padding: '18px 20px', borderRadius: 12, backgroundColor: 'var(--bg-elevated)', border: ch.connected ? '2px solid var(--success)' : '1px solid var(--border-subtle)' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8 }}>
              <span style={{ fontSize: 22 }}>{ch.icon}</span>
              <span style={{ fontSize: 15, fontWeight: 600 }}>{ch.name}</span>
              <span style={{ fontSize: 11, padding: '2px 8px', borderRadius: 10, backgroundColor: ch.connected ? 'var(--success-bg)' : 'var(--bg-glass)', color: ch.connected ? 'var(--success)' : 'var(--text-muted)', fontWeight: ch.connected ? 600 : 400 }}>
                {ch.connected ? t('channels.connected') : t('channels.disconnected')}
              </span>
            </div>
            <p style={{ fontSize: 13, color: 'var(--text-secondary)', margin: '0 0 12px', lineHeight: 1.5 }}>{ch.desc}</p>
            {ch.connected && ch.bot && <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 8 }}>Bot: @{ch.bot.username}</div>}

            {/* 微信扫码区域 */}
            {configuring === ch.id && ch.id === 'wechat' && (
              <div style={{ marginBottom: 12, textAlign: 'center' }}>
                {error && <div style={{ color: saving ? 'var(--accent)' : 'var(--error)', fontSize: 13, marginBottom: 8 }}>{error}</div>}
                {weixinQr && (
                  <div style={{ padding: 16, backgroundColor: '#fff', borderRadius: 12, display: 'inline-block', margin: '8px 0' }}>
                    <QRCodeSVG value={weixinQr} size={200} />
                  </div>
                )}
                {saving && <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>{t('channels.statusWaitingScan')}</div>}
                {!saving && <button onClick={() => { setConfiguring(null); setError(''); setWeixinQr('') }}
                  style={{ padding: '6px 16px', borderRadius: 8, border: '1px solid var(--border-subtle)', fontSize: 13, cursor: 'pointer', marginTop: 8 }}>{t('common.cancel')}</button>}
              </div>
            )}

            {configuring === ch.id && ch.id !== 'wechat' && ch.configFields && (
              <div style={{ marginBottom: 12 }}>
                {error && <div style={{ color: 'var(--error)', fontSize: 12, marginBottom: 8 }}>{error}</div>}
                {ch.configFields.map(f => (
                  <input key={f.key} type={f.type || 'text'} placeholder={f.placeholder}
                    value={formValues[f.key] || ''} onChange={e => setFormValues({ ...formValues, [f.key]: e.target.value })}
                    style={{ width: '100%', padding: '8px 12px', borderRadius: 8, fontSize: 13, border: '1px solid var(--border-subtle)', marginBottom: 8, boxSizing: 'border-box' }} />
                ))}
                <div style={{ display: 'flex', gap: 8 }}>
                  <button onClick={() => handleSetup(ch.id)} disabled={saving}
                    style={{ padding: '6px 16px', borderRadius: 8, backgroundColor: 'var(--accent)', color: '#fff', border: 'none', fontSize: 13, cursor: 'pointer' }}>
                    {saving ? t('channels.btnConnecting') : t('channels.btnConnect')}
                  </button>
                  <button onClick={() => { setConfiguring(null); setError('') }}
                    style={{ padding: '6px 16px', borderRadius: 8, border: '1px solid var(--border-subtle)', fontSize: 13, cursor: 'pointer' }}>{t('common.cancel')}</button>
                </div>
              </div>
            )}

            {configuring !== ch.id && (ch.connected ? (
              <button onClick={() => handleDisconnect(ch.id)} style={{ padding: '6px 14px', borderRadius: 8, border: '1px solid var(--border-subtle)', fontSize: 12, cursor: 'pointer', color: 'var(--error)' }}>{t('channels.btnDisconnect')}</button>
            ) : (
              <button onClick={() => {
                if (ch.id === 'wechat') { handleSetup('wechat'); return }
                ch.configFields ? setConfiguring(ch.id) : toast.info(t('channels.errorNotSupported'))
              }} style={{ padding: '6px 14px', borderRadius: 8, border: '1px solid var(--border-subtle)', fontSize: 12, cursor: 'pointer', color: 'var(--text-accent)' }}>
                {ch.id === 'wechat' ? t('channels.btnScanConnect') : t('channels.btnConfigure')}
              </button>
            ))}

            {/* Agent Bot 列表 */}
            {(() => {
              const chAgents = agentChannels.filter(ac => ac.channelType === ch.id)
              return chAgents.length > 0 ? (
                <div style={{ marginTop: 12, borderTop: '1px solid var(--border-subtle)', paddingTop: 10 }}>
                  <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6 }}>Agent Bot</div>
                  {chAgents.map((ac: any) => {
                    const agent = allAgents.find(a => a.id === ac.agentId)
                    return (
                      <div key={ac.id} style={{
                        display: 'flex', alignItems: 'center', gap: 6, padding: '4px 0', fontSize: 12,
                      }}>
                        <span style={{
                          width: 6, height: 6, borderRadius: '50%',
                          backgroundColor: ac.status === 'running' ? '#22c55e' : ac.status === 'error' ? '#ef4444' : '#9ca3af',
                        }} />
                        <span style={{ fontWeight: 500 }}>{agent?.name || agent?.model || ac.agentId.slice(0, 8)}</span>
                        <span style={{ color: 'var(--text-muted)' }}>{ac.status}</span>
                        <span style={{ flex: 1 }} />
                        <button onClick={async () => {
                          try {
                            await invoke('toggle_agent_channel', { id: ac.id, enabled: !ac.enabled })
                            loadAgentChannels()
                          } catch (e) { toast.error(String(e)) }
                        }} style={{ fontSize: 10, padding: '2px 6px', borderRadius: 3, border: '1px solid var(--border-subtle)', cursor: 'pointer', color: ac.enabled ? 'var(--success)' : 'var(--text-muted)' }}>
                          {ac.enabled ? 'ON' : 'OFF'}
                        </button>
                        <button onClick={async () => {
                          try {
                            await invoke('delete_agent_channel', { id: ac.id })
                            loadAgentChannels()
                          } catch (e) { toast.error(String(e)) }
                        }} style={{ fontSize: 10, padding: '2px 6px', borderRadius: 3, border: '1px solid var(--border-subtle)', cursor: 'pointer', color: 'var(--error)' }}>
                          ×
                        </button>
                      </div>
                    )
                  })}
                </div>
              ) : null
            })()}

            {/* 添加 Agent Bot 按钮 */}
            <button onClick={() => { setAddingChannel({ channelType: ch.id }); setAddAgentId(''); setAddFormValues({}) }}
              style={{ marginTop: 8, padding: '4px 10px', fontSize: 11, borderRadius: 4, border: '1px dashed var(--border-subtle)', cursor: 'pointer', color: 'var(--text-muted)', backgroundColor: 'transparent', width: '100%' }}>
              + {t('channels.addAgentBot')}
            </button>
          </div>
        ))}
      </div>

      {/* 添加 Agent Bot 弹窗 */}
      {(() => {
        const def = addingChannel ? CHANNEL_DEFS.find(d => d.id === addingChannel.channelType) : null
        return (
          <Modal open={!!addingChannel && !!def} onClose={() => setAddingChannel(null)}
            title={def ? `${def.icon} ${t('channels.addAgentBot')} — ${def.name}` : ''}
            footer={
              <>
                <button onClick={() => setAddingChannel(null)}
                  style={{ padding: '8px 16px', borderRadius: 6, border: '1px solid var(--border-subtle)', cursor: 'pointer', fontSize: 13, color: 'var(--text-secondary)' }}>
                  {t('common.cancel')}
                </button>
                <button disabled={!addAgentId} onClick={async () => {
                  if (!addingChannel) return
                  try {
                    await invoke('create_agent_channel', { agentId: addAgentId, channelType: addingChannel.channelType, credentials: addFormValues, displayName: null })
                    toast.success(t('agentChannels.created'))
                    setAddingChannel(null)
                    loadAgentChannels()
                  } catch (e) { toast.error(String(e)) }
                }}
                  style={{ padding: '8px 20px', borderRadius: 6, border: 'none', backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer', fontSize: 13, opacity: addAgentId ? 1 : 0.5 }}>
                  {t('agentChannels.connect')}
                </button>
              </>
            }
          >
            {/* 选择 Agent */}
            <label style={{ fontSize: 12, fontWeight: 500, display: 'block', marginBottom: 4, color: 'var(--text-secondary)' }}>
              {t('channels.selectAgent')}
            </label>
            <Select value={addAgentId} onChange={setAddAgentId}
              placeholder={t('channels.chooseAgent')}
              options={allAgents.map(a => ({ value: a.id, label: a.name || a.model || a.id.slice(0, 8) }))}
              style={{ width: '100%', marginBottom: 12 }} />

            {/* Bot 凭证 */}
            {def?.configFields?.map(f => (
              <div key={f.key} style={{ marginBottom: 10 }}>
                <label style={{ fontSize: 12, fontWeight: 500, display: 'block', marginBottom: 4, color: 'var(--text-secondary)' }}>{f.label}</label>
                <input type={f.type || 'text'} value={addFormValues[f.key] || ''} onChange={e => setAddFormValues(prev => ({ ...prev, [f.key]: e.target.value }))}
                  placeholder={f.placeholder}
                  style={{ width: '100%', padding: '8px 12px', borderRadius: 6, fontSize: 13, border: '1px solid var(--border-subtle)', boxSizing: 'border-box', backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' }} />
              </div>
            ))}
          </Modal>
        )
      })()}
    </div>
  )
}
