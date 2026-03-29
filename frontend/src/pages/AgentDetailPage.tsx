/**
 * Agent 详情页 - 多 Tab 布局
 *
 * Tabs: 对话 | Soul | 工具 | MCP | Skills | 定时任务 | 设置
 * 复用已有的 SoulFileTab, ToolsTab, McpTab, ParamsTab 组件
 */

import { useState, useEffect, useCallback } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'

import ChatTab from '../components/ChatTab'
import SoulFileTab from '../components/SoulFileTab'
import ToolsTab from '../components/ToolsTab'
import ParamsTab from '../components/ParamsTab'
import McpTab from '../components/McpTab'
import ChannelsTab from '../components/ChannelsTab'
import Select from '../components/Select'
import ProviderModelSelector from '../components/ProviderModelSelector'

// ─── Types ───────────────────────────────────────────────────

interface Agent {
  id: string
  name: string
  model: string
  systemPrompt: string
  temperature: number | null
  maxTokens: number | null
  createdAt: number
  updatedAt: number
}

interface Skill {
  name: string
  description: string
  enabled: boolean
  path: string
}

interface ProviderInfo {
  id: string
  name: string
  apiType: string
  enabled: boolean
  apiKey?: string
  baseUrl?: string
  models: Array<{ id: string; name?: string }>
}

interface CronJob {
  id: string
  name: string
  agentId: string
  schedule: { kind: string; expr?: string; secs?: number; tz?: string } | string
  jobType: string
  enabled: boolean
  failStreak: number
  nextRunAt: number | null
  next_run_at?: number | null
  lastRunAt: number | null
}

function formatSchedule(s: CronJob['schedule'], t: (key: string, params?: Record<string, any>) => string): string {
  if (typeof s === 'string') return s
  if (s.kind === 'cron') return `cron: ${s.expr || ''}`
  if (s.kind === 'every') return t('agentDetailSub.everyNMin', { n: (s.secs || 0) / 60 })
  return JSON.stringify(s)
}

interface SubagentRecord {
  id: string
  parentId: string
  name: string
  task: string
  status: string
  result: string | null
  createdAt: number
  finishedAt: number | null
  timeoutSecs: number
}

type TabId = 'chat' | 'soul' | 'tools' | 'mcp' | 'skills' | 'cron' | 'channels' | 'autonomy' | 'plugins' | 'relations' | 'settings' | 'subagents' | 'audit'

const TAB_KEYS: { id: TabId; labelKey: string }[] = [
  { id: 'chat', labelKey: 'agentDetail.tabChat' },
  { id: 'soul', labelKey: 'agentDetail.tabSoul' },
  { id: 'tools', labelKey: 'agentDetail.tabTools' },
  { id: 'mcp', labelKey: 'agentDetail.tabMcp' },
  { id: 'skills', labelKey: 'agentDetail.tabSkills' },
  { id: 'cron', labelKey: 'agentDetail.tabCron' },
  { id: 'channels', labelKey: 'agentDetail.tabChannels' },
  { id: 'autonomy', labelKey: 'agentDetail.tabAutonomy' },
  { id: 'plugins', labelKey: 'agentDetail.tabPlugins' },
  { id: 'relations', labelKey: 'agentDetail.tabRelations' },
  { id: 'subagents', labelKey: 'agentDetail.tabSubagents' },
  { id: 'settings', labelKey: 'agentDetail.tabSettings' },
  { id: 'audit', labelKey: 'agentDetail.tabAudit' },
]

// ─── Main Component ──────────────────────────────────────────

export default function AgentDetailPage() {
  const { agentId } = useParams<{ agentId: string }>()
  const navigate = useNavigate()
  const { t } = useI18n()
  const [agent, setAgent] = useState<Agent | null>(null)
  const [activeTab, setActiveTab] = useState<TabId>('chat')
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!agentId) return
    setActiveTab('chat')
    setAgent(null)
    setLoading(true)
    ;(async () => {
      try {
        const agents = await invoke<Agent[]>('list_agents')
        const found = agents.find((a) => a.id === agentId)
        if (found) setAgent(found)
      } catch (e) {
        console.error(e)
        toast.error(t('common.error') + ': ' + String(e))
      } finally {
        setLoading(false)
      }
    })()
  }, [agentId])

  if (loading) return <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('common.loading')}</div>
  if (!agent || !agentId) return <div style={{ padding: 40, textAlign: 'center' }}>{t('agentDetail.notFound')}</div>

  return (
    <div style={{ display: 'flex', flexDirection: 'column', position: 'absolute', inset: 0 }}>
      {/* 顶部：返回 + Agent 信息 */}
      <div style={{ padding: '12px 24px', borderBottom: '1px solid var(--border-subtle)', display: 'flex', alignItems: 'center', gap: 16 }}>
        <button
          onClick={() => navigate('/agents')}
          style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: 18, color: 'var(--text-secondary)', padding: '4px 8px' }}
        >
          ←
        </button>
        <div>
          <h2 style={{ margin: 0, fontSize: 18, fontWeight: 600 }}>{agent.name}</h2>
          <span style={{
            fontSize: 12, padding: '2px 8px', borderRadius: 4,
            backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)',
          }}>
            {agent.model}
          </span>
        </div>
      </div>

      {/* Tab 栏 */}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border-subtle)', padding: '0 24px', overflowX: 'auto', flexShrink: 0 }}>
        {TAB_KEYS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            style={{
              padding: '10px 16px', border: 'none', cursor: 'pointer',
              fontSize: 14, backgroundColor: 'transparent', whiteSpace: 'nowrap',
              color: activeTab === tab.id ? 'var(--accent)' : 'var(--text-secondary)',
              borderBottom: activeTab === tab.id ? '2px solid var(--accent)' : '2px solid transparent',
              fontWeight: activeTab === tab.id ? 600 : 400,
            }}
          >
            {t(tab.labelKey)}
          </button>
        ))}
      </div>

      {/* Tab 内容 */}
      <div style={{ flex: 1, overflow: activeTab === 'chat' ? 'hidden' : 'auto', minHeight: 0 }}>
        {activeTab === 'chat' && <ChatTab agentId={agentId} />}
        {activeTab === 'soul' && <div style={{ padding: 16 }}><SoulFileTab agentId={agentId} /></div>}
        {activeTab === 'tools' && <div style={{ padding: 16 }}><ToolsTab agentId={agentId} /></div>}
        {activeTab === 'mcp' && <div style={{ padding: 16 }}><McpTab agentId={agentId} /></div>}
        {activeTab === 'skills' && <SkillsTab agentId={agentId} />}
        {activeTab === 'cron' && <CronTab agentId={agentId} />}
        {activeTab === 'channels' && <ChannelsTab agentId={agentId} />}
        {activeTab === 'autonomy' && <AutonomyTab agentId={agentId} />}
        {activeTab === 'plugins' && <PluginsTab />}
        {activeTab === 'relations' && <RelationsTab agentId={agentId} />}
        {activeTab === 'subagents' && <SubagentsTab agentId={agentId} />}
        {activeTab === 'audit' && <AuditTab agentId={agentId} />}
        {activeTab === 'settings' && <SettingsTab key={agent.id} agentId={agentId} agent={agent} onUpdate={setAgent} onDelete={() => navigate('/agents')} />}
      </div>
    </div>
  )
}

// ─── Skills Tab ──────────────────────────────────────────────

function SkillsTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [skills, setSkills] = useState<Skill[]>([])
  const [installUrl, setInstallUrl] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const loadSkills = useCallback(async () => {
    try {
      const result = await invoke<Skill[]>('list_skills', { agentId })
      setSkills(result)
    } catch (e) { setError(String(e)) }
  }, [agentId])

  useEffect(() => { loadSkills() }, [loadSkills])

  const handleInstall = async () => {
    if (!installUrl.trim()) return
    setLoading(true)
    setError('')
    try {
      await invoke('install_skill', { agentId, filePath: installUrl.trim() })
      setInstallUrl('')
      loadSkills()
    } catch (e) { setError(String(e)) }
    finally { setLoading(false) }
  }

  const handleToggle = async (name: string, enabled: boolean) => {
    try {
      await invoke('toggle_skill', { agentId, skillName: name, enabled: !enabled })
      loadSkills()
    } catch (e) { setError(String(e)) }
  }

  const handleRemove = async (name: string) => {
    try {
      await invoke('remove_skill', { agentId, skillName: name })
      loadSkills()
    } catch (e) { setError(String(e)) }
  }

  return (
    <div style={{ padding: 20, maxWidth: 600 }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 16 }}>{t('agentDetailSub.skillsTitle')}</h3>
      {error && <div style={{ padding: 8, backgroundColor: 'var(--error-bg)', color: 'var(--error)', borderRadius: 6, marginBottom: 12, fontSize: 13 }}>{error}</div>}

      <div style={{ display: 'flex', gap: 8, marginBottom: 20 }}>
        <input
          value={installUrl}
          onChange={(e) => setInstallUrl(e.target.value)}
          placeholder={t('agentDetailSub.skillsPlaceholder')}
          style={{ flex: 1, padding: '8px 12px', border: '1px solid var(--border-subtle)', borderRadius: 6, fontSize: 13 }}
        />
        <button
          onClick={handleInstall}
          disabled={loading || !installUrl.trim()}
          style={{
            padding: '8px 16px', backgroundColor: 'var(--accent)', color: 'white',
            border: 'none', borderRadius: 6, cursor: 'pointer', fontSize: 13,
            opacity: loading || !installUrl.trim() ? 0.6 : 1,
          }}
        >
          {loading ? t('agentDetailSub.skillsInstalling') : t('agentDetailSub.skillsInstall')}
        </button>
      </div>

      {skills.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)' }}>{t('agentDetailSub.skillsEmpty')}</div>
      ) : (
        skills.map((skill) => (
          <div key={skill.name} style={{
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
            padding: '12px 16px', border: '1px solid var(--border-subtle)', borderRadius: 8, marginBottom: 8,
          }}>
            <div style={{ flex: 1 }}>
              <div style={{ fontWeight: 500, fontSize: 14 }}>{skill.name}</div>
              {skill.description && <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>{skill.description}</div>}
            </div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              <button
                onClick={() => handleToggle(skill.name, skill.enabled)}
                style={{
                  padding: '4px 10px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                  border: '1px solid var(--border-subtle)', backgroundColor: skill.enabled ? 'var(--success-bg)' : 'var(--bg-glass)',
                  color: skill.enabled ? 'var(--success)' : 'var(--text-secondary)',
                }}
              >
                {skill.enabled ? t('agentDetailSub.skillsEnabled') : t('agentDetailSub.skillsDisabled')}
              </button>
              <button
                onClick={() => handleRemove(skill.name)}
                style={{
                  padding: '4px 10px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                  border: '1px solid #fecaca', backgroundColor: 'var(--error-bg)', color: 'var(--error)',
                }}
              >
                {t('common.delete')}
              </button>
            </div>
          </div>
        ))
      )}
    </div>
  )
}

// ─── Cron Tab ────────────────────────────────────────────────

function CronTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [jobs, setJobs] = useState<CronJob[]>([])
  const [error, setError] = useState('')

  const loadJobs = useCallback(async () => {
    try {
      const result = await invoke<CronJob[]>('list_cron_jobs', { agentId })
      setJobs(result)
    } catch (e) { setError(String(e)) }
  }, [agentId])

  useEffect(() => { loadJobs() }, [loadJobs])

  const handleTrigger = async (jobId: string) => {
    try {
      await invoke('trigger_cron_job', { jobId })
      loadJobs()
    } catch (e) { setError(String(e)) }
  }

  const handleToggle = async (jobId: string, enabled: boolean) => {
    try {
      if (enabled) await invoke('pause_cron_job', { jobId })
      else await invoke('resume_cron_job', { jobId })
      loadJobs()
    } catch (e) { setError(String(e)) }
  }

  const handleDelete = async (jobId: string) => {
    try {
      await invoke('delete_cron_job', { jobId })
      loadJobs()
    } catch (e) { setError(String(e)) }
  }

  const formatTime = (ts: number | null) => {
    if (!ts) return '-'
    return new Date(ts).toLocaleString('zh-CN')
  }

  return (
    <div style={{ padding: 20, maxWidth: 800 }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 16 }}>{t('agentDetailSub.cronTitle')}</h3>
      {error && <div style={{ padding: 8, backgroundColor: 'var(--error-bg)', color: 'var(--error)', borderRadius: 6, marginBottom: 12, fontSize: 13 }}>{error}</div>}

      {jobs.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)' }}>{t('agentDetailSub.cronEmpty')}</div>
      ) : (
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
          <thead>
            <tr style={{ borderBottom: '2px solid var(--border-subtle)' }}>
              <th style={{ textAlign: 'left', padding: '8px 12px' }}>{t('agentDetailSub.cronName')}</th>
              <th style={{ textAlign: 'left', padding: '8px 12px' }}>{t('agentDetailSub.cronPlan')}</th>
              <th style={{ textAlign: 'left', padding: '8px 12px' }}>{t('agentDetailSub.cronNextRun')}</th>
              <th style={{ textAlign: 'left', padding: '8px 12px' }}>{t('agentDetailSub.cronStatus')}</th>
              <th style={{ textAlign: 'right', padding: '8px 12px' }}>{t('agentDetailSub.cronActions')}</th>
            </tr>
          </thead>
          <tbody>
            {jobs.map((job) => (
              <tr key={job.id} style={{ borderBottom: '1px solid var(--border-subtle)' }}>
                <td style={{ padding: '10px 12px' }}>{job.name}</td>
                <td style={{ padding: '10px 12px', fontFamily: 'monospace', fontSize: 12 }}>{formatSchedule(job.schedule, t)}</td>
                <td style={{ padding: '10px 12px', fontSize: 12 }}>{formatTime(job.nextRunAt ?? job.next_run_at ?? null)}</td>
                <td style={{ padding: '10px 12px' }}>
                  <span style={{
                    padding: '2px 8px', borderRadius: 4, fontSize: 11,
                    backgroundColor: job.enabled ? 'var(--success-bg)' : 'var(--bg-glass)',
                    color: job.enabled ? 'var(--success)' : 'var(--text-secondary)',
                  }}>
                    {job.enabled ? t('agentDetailSub.cronRunning') : t('agentDetailSub.cronPaused')}
                  </span>
                </td>
                <td style={{ padding: '10px 12px', textAlign: 'right' }}>
                  <div style={{ display: 'flex', gap: 4, justifyContent: 'flex-end' }}>
                    <button onClick={() => handleTrigger(job.id)} style={{ padding: '3px 8px', fontSize: 11, border: '1px solid var(--border-subtle)', borderRadius: 4, cursor: 'pointer', backgroundColor: 'var(--bg-elevated)' }}>{t('agentDetailSub.cronTrigger')}</button>
                    <button onClick={() => handleToggle(job.id, job.enabled)} style={{ padding: '3px 8px', fontSize: 11, border: '1px solid var(--border-subtle)', borderRadius: 4, cursor: 'pointer', backgroundColor: 'var(--bg-elevated)' }}>{job.enabled ? t('agentDetailSub.cronPause') : t('agentDetailSub.cronResume')}</button>
                    <button onClick={() => handleDelete(job.id)} style={{ padding: '3px 8px', fontSize: 11, border: '1px solid #fecaca', borderRadius: 4, cursor: 'pointer', backgroundColor: 'var(--error-bg)', color: 'var(--error)' }}>{t('common.delete')}</button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

// ─── Settings Tab ────────────────────────────────────────────

function SettingsTab({ agentId, agent, onUpdate, onDelete }: {
  agentId: string
  agent: Agent
  onUpdate: (a: Agent) => void
  onDelete: () => void
}) {
  const { t } = useI18n()
  const [name, setName] = useState(agent.name)
  const [model, setModel] = useState(agent.model)
  const [temperature, setTemperature] = useState(agent.temperature ?? 0.7)
  const [maxTokens, setMaxTokens] = useState(agent.maxTokens ?? 2048)
  const [saving, setSaving] = useState(false)
  const [deleteConfirm, setDeleteConfirm] = useState(false)
  const [msg, setMsg] = useState('')

  const handleSave = async () => {
    setSaving(true)
    setMsg('')
    try {
      await invoke('update_agent', {
        agentId,
        name: name !== agent.name ? name : null,
        model: model !== agent.model ? model : null,
        temperature: temperature !== agent.temperature ? temperature : null,
        maxTokens: maxTokens !== agent.maxTokens ? maxTokens : null,
      })
      onUpdate({ ...agent, name, model, temperature, maxTokens })
      setMsg(t('settings.successSaved'))
    } catch (e) { setMsg(String(e)) }
    finally { setSaving(false) }
  }

  const handleDelete = async () => {
    try {
      await invoke('delete_agent', { agentId })
      onDelete()
    } catch (e) { setMsg(String(e)) }
  }

  const inputStyle: React.CSSProperties = {
    width: '100%', padding: '10px 14px', border: '1px solid var(--border-subtle)',
    borderRadius: 10, fontSize: 14, boxSizing: 'border-box',
    backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
    outline: 'none', transition: 'border-color 0.15s',
  }
  const cardStyle: React.CSSProperties = {
    padding: '20px', borderRadius: 14, border: '1px solid var(--border-subtle)',
    backgroundColor: 'var(--bg-elevated)', marginBottom: 16,
  }

  return (
    <div style={{ padding: 20, maxWidth: 560 }}>
      {msg && <div style={{ padding: '10px 14px', backgroundColor: msg === t('settings.successSaved') ? 'var(--success-bg)' : 'var(--error-bg)', color: msg === t('settings.successSaved') ? 'var(--success)' : 'var(--error)', borderRadius: 10, marginBottom: 16, fontSize: 13 }}>{msg}</div>}

      {/* 基本信息卡片 */}
      <div style={cardStyle}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
          </svg>
          {t('agentDetailSub.settingsTitle')}
        </div>

        <div style={{ marginBottom: 14 }}>
          <label style={{ display: 'block', fontSize: 12, fontWeight: 500, marginBottom: 6, color: 'var(--text-muted)' }}>{t('common.name')}</label>
          <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} />
        </div>

        <div style={{ marginBottom: 14 }}>
          <ProviderModelSelector
            value={model}
            onChange={setModel}
            requireKey={false}
          />
        </div>
      </div>

      {/* 参数调整卡片 */}
      <div style={cardStyle}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>
          </svg>
          Parameters
        </div>

        <div style={{ marginBottom: 18 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-muted)' }}>Temperature</label>
            <span style={{ fontSize: 20, fontWeight: 700, color: 'var(--text-primary)' }}>{temperature.toFixed(1)}</span>
          </div>
          <input type="range" min="0" max="2" step="0.1" value={temperature} onChange={(e) => setTemperature(parseFloat(e.target.value))}
            style={{ width: '100%', accentColor: 'var(--accent)' }} />
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
            <span>{t('agentCreate.tempPrecise')}</span><span>{t('agentCreate.tempBalanced')}</span><span>{t('agentCreate.tempCreative')}</span>
          </div>
        </div>

        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <label style={{ fontSize: 12, fontWeight: 500, color: 'var(--text-muted)' }}>Max Tokens</label>
            <span style={{ fontSize: 20, fontWeight: 700, color: 'var(--text-primary)' }}>{maxTokens.toLocaleString()}</span>
          </div>
          <input type="range" min="256" max="8192" step="256" value={maxTokens} onChange={(e) => setMaxTokens(parseInt(e.target.value))}
            style={{ width: '100%', accentColor: 'var(--accent)' }} />
        </div>
      </div>

      {/* 保存按钮 */}
      <button onClick={handleSave} disabled={saving} style={{
        width: '100%', padding: '12px', backgroundColor: 'var(--accent)', color: 'white',
        border: 'none', borderRadius: 10, cursor: 'pointer', fontSize: 14, fontWeight: 600,
        marginBottom: 16, opacity: saving ? 0.6 : 1, transition: 'opacity 0.15s',
      }}>
        {saving ? t('common.saving') : t('common.save')}
      </button>

      {/* 导出/导入卡片 */}
      <div style={{ ...cardStyle, display: 'flex', gap: 10 }}>
        <button onClick={async () => {
          try {
            const bundle = await invoke<string>('export_agent_bundle', { agentId: agent?.id || '' })
            const blob = new Blob([bundle], { type: 'application/json' })
            const url = URL.createObjectURL(blob)
            const a = document.createElement('a')
            a.href = url; a.download = `agent-${(agent?.name || 'export').replace(/\s+/g, '-')}.json`; a.click()
            URL.revokeObjectURL(url)
            toast.success('Agent exported')
          } catch (e) { toast.error(String(e)) }
        }} style={{
          flex: 1, padding: '10px 16px', backgroundColor: 'var(--bg-glass)', color: 'var(--text-primary)',
          border: '1px solid var(--border-subtle)', borderRadius: 10, cursor: 'pointer', fontSize: 13,
          display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6,
        }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          Export
        </button>
        <button onClick={() => {
          const input = document.createElement('input')
          input.type = 'file'; input.accept = '.json'
          input.onchange = async () => {
            if (!input.files?.[0]) return
            const text = await input.files[0].text()
            try {
              const result = await invoke<string>('import_agent_bundle', { bundleJson: text })
              toast.success('Agent imported: ' + result)
              window.location.reload()
            } catch (e) { toast.error(String(e)) }
          }
          input.click()
        }} style={{
          flex: 1, padding: '10px 16px', backgroundColor: 'var(--bg-glass)', color: 'var(--text-primary)',
          border: '1px solid var(--border-subtle)', borderRadius: 10, cursor: 'pointer', fontSize: 13,
          display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6,
        }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>
          </svg>
          Import
        </button>
      </div>

      {/* 危险区域 */}
      <div style={{ ...cardStyle, borderColor: 'rgba(239,68,68,0.2)', backgroundColor: 'rgba(239,68,68,0.03)' }}>
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8, color: 'var(--error)', display: 'flex', alignItems: 'center', gap: 8 }}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--error)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>
          </svg>
          {t('agentDetailSub.dangerZone')}
        </div>
        <p style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 14, marginTop: 0 }}>{t('agentDetailSub.dangerDesc')}</p>
        {!deleteConfirm ? (
          <button onClick={() => setDeleteConfirm(true)} style={{
            padding: '8px 16px', backgroundColor: 'transparent', color: 'var(--error)',
            border: '1px solid rgba(239,68,68,0.3)', borderRadius: 8, cursor: 'pointer', fontSize: 13,
          }}>
            {t('agentDetailSub.deleteAgent')}
          </button>
        ) : (
          <div style={{ display: 'flex', gap: 8 }}>
            <button onClick={handleDelete} style={{
              padding: '8px 20px', backgroundColor: 'var(--error)', color: 'white',
              border: 'none', borderRadius: 8, cursor: 'pointer', fontSize: 13, fontWeight: 600,
            }}>
              {t('agents.btnConfirmDelete')}
            </button>
            <button onClick={() => setDeleteConfirm(false)} style={{
              padding: '8px 16px', backgroundColor: 'var(--bg-glass)', color: 'var(--text-primary)',
              border: '1px solid var(--border-subtle)', borderRadius: 8, cursor: 'pointer', fontSize: 13,
            }}>
              {t('common.cancel')}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}

// ─── Audit Tab ───────────────────────────────────────────────

interface AuditEntry {
  id: string
  toolName: string
  arguments: string
  result: string | null
  success: boolean
  policyDecision: string
  policySource: string
  durationMs: number
  createdAt: number
}

function RelationsTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [relations, setRelations] = useState<any[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [targetId, setTargetId] = useState('')
  const [relType, setRelType] = useState('collaborator')
  const [loading, setLoading] = useState(true)

  const load = async () => {
    try {
      const [rels, agentList] = await Promise.all([
        invoke<any[]>('get_agent_relations', { agentId }),
        invoke<any[]>('list_agents'),
      ])
      setRelations(rels)
      setAgents(agentList.filter((a: any) => a.id !== agentId))
    } catch (e) { console.error('loadRelations failed:', e) }
    setLoading(false)
  }

  useEffect(() => { load() }, [agentId])

  const handleAdd = async () => {
    if (!targetId) return
    try {
      await invoke('create_agent_relation', { fromId: agentId, toId: targetId, relationType: relType })
      await load()
      setTargetId('')
    } catch (e) { toast.error(String(e)) }
  }

  const handleDelete = async (id: string) => {
    try {
      await invoke('delete_agent_relation', { relationId: id })
      await load()
    } catch (e) { toast.error(String(e)) }
  }

  const RELATION_TYPES = [
    { value: 'collaborator', label: t('agentDetail.relCollaborator') },
    { value: 'supervisor', label: t('agentDetail.relSupervisor') },
    { value: 'subordinate', label: t('agentDetail.relSubordinate') },
    { value: 'peer', label: t('agentDetail.relPeer') },
  ]

  const getAgentName = (id: string) => agents.find(a => a.id === id)?.name || id.slice(0, 8)

  if (loading) return <div style={{ padding: 20, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: 20 }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 16, fontWeight: 600 }}>{t('agentDetail.relTitle')}</h3>

      {/* 添加关系 */}
      <div style={{
        display: 'flex', gap: 8, marginBottom: 20, padding: 12,
        borderRadius: 10, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
      }}>
        <Select value={targetId} onChange={setTargetId}
          placeholder={t('agentDetail.relSelectAgent')}
          options={agents.map(a => ({ value: a.id, label: a.name }))}
          style={{ flex: 1 }} />
        <Select value={relType} onChange={setRelType}
          options={RELATION_TYPES.map(rt => ({ value: rt.value, label: rt.label }))}
          style={{ minWidth: 100 }} />
        <button onClick={handleAdd} disabled={!targetId}
          style={{ padding: '6px 16px', borderRadius: 6, fontSize: 13, border: 'none', backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer' }}>
          {t('agentDetail.relAdd')}
        </button>
      </div>

      {/* 关系列表 */}
      {relations.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)', fontSize: 13 }}>
          {t('agentDetail.relEmpty')}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {relations.map(r => (
            <div key={r.id} style={{
              display: 'flex', alignItems: 'center', gap: 12, padding: '10px 14px',
              borderRadius: 8, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)',
            }}>
              <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-muted)' }}>--</span>
              <span style={{ fontSize: 13, fontWeight: 500 }}>
                {r.fromId === agentId ? getAgentName(r.toId) : getAgentName(r.fromId)}
              </span>
              <span style={{
                fontSize: 11, padding: '2px 8px', borderRadius: 10,
                backgroundColor: 'var(--accent-bg)', color: 'var(--text-accent)',
              }}>
                {r.relationType}
              </span>
              <span style={{ flex: 1 }} />
              <button onClick={() => handleDelete(r.id)}
                style={{ fontSize: 11, color: 'var(--error)', background: 'none', border: 'none', cursor: 'pointer' }}>
                {t('common.delete')}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function AuditTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [entries, setEntries] = useState<AuditEntry[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    (async () => {
      try {
        const result = await invoke<AuditEntry[]>('get_audit_log', { agentId, limit: 100 })
        setEntries(result)
      } catch (e) { console.error(e) }
      finally { setLoading(false) }
    })()
  }, [agentId])

  if (loading) return <div style={{ padding: 20, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  // 按日期分组
  const grouped: Record<string, AuditEntry[]> = {}
  entries.forEach(e => {
    const day = new Date(e.createdAt).toLocaleDateString('zh-CN')
    if (!grouped[day]) grouped[day] = []
    grouped[day].push(e)
  })

  const successCount = entries.filter(e => e.success).length
  const failCount = entries.length - successCount

  return (
    <div style={{ padding: 20, maxWidth: 800 }}>
      {/* 标题 + 统计 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 20 }}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/>
        </svg>
        <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>{t('agentDetailSub.auditTitle')}</h3>
        <span style={{ flex: 1 }} />
        {entries.length > 0 && (
          <div style={{ display: 'flex', gap: 8, fontSize: 12 }}>
            <span style={{ color: 'var(--success)', display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: 'var(--success)' }} />
              {successCount}
            </span>
            <span style={{ color: 'var(--error)', display: 'flex', alignItems: 'center', gap: 4 }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: 'var(--error)' }} />
              {failCount}
            </span>
            <span style={{ color: 'var(--text-muted)' }}>{entries.length} total</span>
          </div>
        )}
      </div>

      {entries.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 60, color: 'var(--text-muted)' }}>
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.3, marginBottom: 12 }}>
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/>
          </svg>
          <div>{t('agentDetailSub.auditEmpty')}</div>
        </div>
      ) : (
        Object.entries(grouped).map(([day, dayEntries]) => (
          <div key={day} style={{ marginBottom: 20 }}>
            {/* 日期分割线 */}
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
              <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>{day}</span>
              <div style={{ flex: 1, height: 1, backgroundColor: 'var(--border-subtle)' }} />
              <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>{dayEntries.length}</span>
            </div>

            {/* 时间轴条目 */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {dayEntries.map((entry) => (
                <div key={entry.id} style={{
                  display: 'flex', alignItems: 'center', gap: 10,
                  padding: '10px 14px', borderRadius: 10,
                  backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
                  transition: 'background-color 0.15s',
                }}>
                  {/* 状态灯 */}
                  <div style={{
                    width: 8, height: 8, borderRadius: '50%', flexShrink: 0,
                    backgroundColor: entry.success ? 'var(--success)' : 'var(--error)',
                    boxShadow: entry.success ? '0 0 6px var(--success)' : '0 0 6px var(--error)',
                  }} />

                  {/* 时间 */}
                  <span style={{ fontSize: 11, color: 'var(--text-muted)', whiteSpace: 'nowrap', width: 50, flexShrink: 0 }}>
                    {new Date(entry.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
                  </span>

                  {/* 工具名 */}
                  <span style={{
                    fontSize: 13, fontFamily: "'SF Mono', Monaco, monospace", fontWeight: 500,
                    color: 'var(--text-primary)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                  }}>
                    {entry.toolName}
                  </span>

                  {/* 策略 badge */}
                  <span style={{
                    padding: '2px 8px', borderRadius: 9999, fontSize: 10, fontWeight: 500, flexShrink: 0,
                    backgroundColor: entry.policyDecision === 'allowed' ? 'var(--success-bg)' : 'var(--error-bg)',
                    color: entry.policyDecision === 'allowed' ? 'var(--success)' : 'var(--error)',
                    border: `1px solid ${entry.policyDecision === 'allowed' ? 'rgba(34,197,94,0.2)' : 'rgba(239,68,68,0.2)'}`,
                  }}>
                    {entry.policyDecision}
                  </span>

                  {/* 来源 */}
                  <span style={{ fontSize: 10, color: 'var(--text-muted)', flexShrink: 0 }}>{entry.policySource}</span>

                  {/* 耗时 */}
                  <span style={{
                    fontSize: 11, fontFamily: "'SF Mono', Monaco, monospace",
                    color: (entry.durationMs || 0) > 100 ? 'var(--warning)' : 'var(--text-muted)',
                    flexShrink: 0, width: 50, textAlign: 'right',
                  }}>
                    {entry.durationMs}ms
                  </span>
                </div>
              ))}
            </div>
          </div>
        ))
      )}
    </div>
  )
}

// ─── Subagents Tab ───────────────────────────────────────────

function SubagentsTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [subagents, setSubagents] = useState<SubagentRecord[]>([])
  const [loading, setLoading] = useState(true)

  const loadSubagents = useCallback(async () => {
    try {
      const result = await invoke<SubagentRecord[]>('list_subagents', { agentId })
      setSubagents(result)
    } catch (e) { console.error(e) }
    finally { setLoading(false) }
  }, [agentId])

  useEffect(() => { loadSubagents() }, [loadSubagents])

  const handleCancel = async (subagentId: string) => {
    try {
      await invoke('cancel_subagent', { subagentId })
      loadSubagents()
    } catch (e) { console.error(e); toast.error(t('common.error') + ': ' + String(e)) }
  }

  const statusColor = (status: string) => {
    if (status === 'Running') return { bg: 'var(--accent-bg)', color: 'var(--accent)' }
    if (status === 'Completed') return { bg: 'var(--success-bg)', color: 'var(--success)' }
    if (status.startsWith('Failed')) return { bg: 'var(--error-bg)', color: 'var(--error)' }
    if (status === 'Timeout') return { bg: '#fef3c7', color: '#d97706' }
    if (status === 'Cancelled') return { bg: 'var(--bg-glass)', color: 'var(--text-secondary)' }
    return { bg: 'var(--bg-glass)', color: 'var(--text-secondary)' }
  }

  if (loading) return <div style={{ padding: 20, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: 20, maxWidth: 800 }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 16 }}>{t('agentDetailSub.subagentsTitle')}</h3>
      <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 16 }}>
        {t('agentDetailSub.subagentsDesc')}
      </p>

      {subagents.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)' }}>{t('agentDetailSub.subagentsEmpty')}</div>
      ) : (
        subagents.map((sa) => {
          const sc = statusColor(sa.status)
          return (
            <div key={sa.id} style={{
              border: '1px solid var(--border-subtle)', borderRadius: 8, padding: 16, marginBottom: 12,
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
                <div style={{ fontWeight: 600, fontSize: 14 }}>{sa.name}</div>
                <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                  <span style={{
                    padding: '2px 8px', borderRadius: 4, fontSize: 12,
                    backgroundColor: sc.bg, color: sc.color,
                  }}>
                    {sa.status}
                  </span>
                  {sa.status === 'Running' && (
                    <button
                      onClick={() => handleCancel(sa.id)}
                      style={{
                        padding: '2px 8px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                        border: '1px solid #fecaca', backgroundColor: 'var(--error-bg)', color: 'var(--error)',
                      }}
                    >
                      {t('agentDetailSub.subagentsCancel')}
                    </button>
                  )}
                </div>
              </div>
              <div style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 4 }}>{sa.task}</div>
              {sa.result && (
                <div style={{
                  fontSize: 12, backgroundColor: 'var(--bg-glass)', padding: 8, borderRadius: 4,
                  marginTop: 8, whiteSpace: 'pre-wrap', maxHeight: 100, overflow: 'auto',
                }}>
                  {sa.result}
                </div>
              )}
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 8 }}>
                {t('agentDetailSub.subagentsCreated')}: {new Date(sa.createdAt).toLocaleString('zh-CN')}
                {sa.finishedAt && ` · ${t('agentDetailSub.subagentsFinished')}: ${new Date(sa.finishedAt).toLocaleString('zh-CN')}`}
              </div>
            </div>
          )
        })
      )}

      {/* Agent 间消息 */}
      <AgentMessagesPanel agentId={agentId} />
    </div>
  )
}

/** Agent 间消息面板 */
function AgentMessagesPanel({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [messages, setMessages] = useState<any[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [targetId, setTargetId] = useState('')
  const [content, setContent] = useState('')
  const [sending, setSending] = useState(false)

  const load = useCallback(async () => {
    try {
      const [msgs, agentList] = await Promise.all([
        invoke<any[]>('get_agent_mailbox', { agentId }),
        invoke<any[]>('list_agents'),
      ])
      setMessages(msgs)
      setAgents(agentList.filter((a: any) => a.id !== agentId))
    } catch (e) { console.error('loadMailboxPanel failed:', e) }
  }, [agentId])

  // 切换 agent 时清空消息
  useEffect(() => { setMessages([]) }, [agentId])

  useEffect(() => { load() }, [load])

  // 定期拉取新消息
  useEffect(() => {
    const timer = setInterval(async () => {
      try {
        const msgs = await invoke<any[]>('get_agent_mailbox', { agentId })
        if (msgs.length > 0) setMessages(prev => [...prev, ...msgs])
      } catch (e) { console.error('pollMailbox failed:', e) }
    }, 5000)
    return () => clearInterval(timer)
  }, [agentId])

  const handleSend = async () => {
    if (!targetId || !content.trim()) return
    setSending(true)
    try {
      await invoke('send_agent_message', { fromId: agentId, toId: targetId, content: content.trim() })
      setMessages(prev => [...prev, { from: agentId, to: targetId, content: content.trim(), timestamp: Date.now() }])
      setContent('')
      toast.success(t('agentDetailSub.messageSent'))
    } catch (e) { toast.error(String(e)) }
    setSending(false)
  }

  const getName = (id: string) => agents.find(a => a.id === id)?.name || id.slice(0, 8)

  return (
    <div style={{ marginTop: 32 }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {t('agentDetailSub.messagesTitle')}
      </h3>

      {/* 发送消息 */}
      <div style={{
        display: 'flex', gap: 8, marginBottom: 16, padding: 12,
        borderRadius: 10, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
      }}>
        <Select value={targetId} onChange={setTargetId}
          placeholder={t('agentDetailSub.messagesSelectTarget')}
          options={agents.map(a => ({ value: a.id, label: a.name }))}
          style={{ width: 140 }} />
        <input
          value={content} onChange={e => setContent(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && handleSend()}
          placeholder={t('agentDetailSub.messagesPlaceholder')}
          style={{ flex: 1, padding: '6px 10px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
        />
        <button onClick={handleSend} disabled={sending || !targetId || !content.trim()}
          style={{ padding: '6px 16px', borderRadius: 6, fontSize: 13, border: 'none', backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer' }}>
          {t('agentDetailSub.messagesSend')}
        </button>
      </div>

      {/* 消息列表 */}
      {messages.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 24, color: 'var(--text-muted)', fontSize: 12 }}>
          {t('agentDetailSub.messagesEmpty')}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 300, overflow: 'auto' }}>
          {messages.map((msg, i) => (
            <div key={i} style={{
              padding: '8px 12px', borderRadius: 8,
              backgroundColor: msg.from === agentId ? 'var(--user-bubble)' : 'var(--assistant-bubble)',
              border: '1px solid var(--border-subtle)',
            }}>
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 4 }}>
                {msg.from === agentId ? `→ ${getName(msg.to)}` : `← ${getName(msg.from)}`}
                <span style={{ marginLeft: 8 }}>{new Date(msg.timestamp).toLocaleTimeString()}</span>
              </div>
              <div style={{ fontSize: 13, color: 'var(--text-primary)' }}>{msg.content}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

// ─── Plugins Tab ─────────────────────────────────────────────

interface SystemPlugin {
  id: string; name: string; description: string; pluginType: string
  builtin: boolean; icon: string; defaultEnabled: boolean
}

function PluginsTab() {
  const { t } = useI18n()
  const [plugins, setPlugins] = useState<SystemPlugin[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    (async () => {
      try {
        const result = await invoke<SystemPlugin[]>('list_system_plugins')
        setPlugins(result)
      } catch (e) { console.error(e) }
      finally { setLoading(false) }
    })()
  }, [])

  if (loading) return <div style={{ padding: 20, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  // 按类型分组
  const grouped: Record<string, SystemPlugin[]> = {}
  for (const p of plugins) {
    if (!grouped[p.pluginType]) grouped[p.pluginType] = []
    grouped[p.pluginType].push(p)
  }

  return (
    <div style={{ padding: 20, maxWidth: 700 }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 16 }}>{t('agentDetail.pluginsTitle')}</h3>
      <p style={{ fontSize: 13, color: 'var(--text-secondary)', marginBottom: 16 }}>
        {t('agentDetail.pluginsDesc')}
      </p>

      {Object.entries(grouped).map(([type, items]) => (
        <div key={type} style={{ marginBottom: 16 }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 8 }}>{type}</div>
          {items.map(p => (
            <div key={p.id} style={{
              display: 'flex', alignItems: 'center', gap: 10, padding: '8px 0',
              borderBottom: '1px solid var(--border-subtle)',
            }}>
              <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-muted)' }}>{p.icon || '+'}</span>
              <div style={{ flex: 1 }}>
                <span style={{ fontSize: 13, fontWeight: 500 }}>{p.name}</span>
                {p.builtin && <span style={{ fontSize: 10, marginLeft: 6, padding: '1px 5px', borderRadius: 3, backgroundColor: '#6366F1', color: '#fff' }}>{t('agentDetailSub.builtinLabel')}</span>}
              </div>
              <div style={{
                width: 8, height: 8, borderRadius: '50%',
                backgroundColor: p.defaultEnabled ? 'var(--success)' : 'var(--border-subtle)',
              }} />
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

// ─── Autonomy Tab ────────────────────────────────────────────

interface AutonomyConfigData {
  default_level: string
  overrides: Record<string, string>
}

const LEVEL_COLORS: Record<string, { color: string; bg: string; icon: string }> = {
  L1Confirm: { color: '#ef4444', bg: 'rgba(239,68,68,0.1)', icon: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z' },
  L2Notify: { color: '#f59e0b', bg: 'rgba(245,158,11,0.1)', icon: 'M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9v2 M12 15h.01' },
  L3Autonomous: { color: '#22c55e', bg: 'rgba(34,197,94,0.1)', icon: 'M22 11.08V12a10 10 0 1 1-5.93-9.14 M22 4L12 14.01l-3-3' },
}

const GROUP_ICONS: Record<string, string> = {
  groupSafe: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z',
  groupRead: 'M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 12m-3 0a3 3 0 1 0 6 0 3 3 0 1 0-6 0',
  groupWrite: 'M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z',
  groupExec: 'M4 17l6-6-6-6 M12 19h8',
}

const TOOL_GROUP_TOOLS = [
  { key: 'groupSafe', tools: ['calculator', 'datetime', 'memory_read'] },
  { key: 'groupRead', tools: ['file_read', 'file_list', 'code_search', 'web_search', 'web_fetch'] },
  { key: 'groupWrite', tools: ['file_write', 'file_edit', 'diff_edit', 'memory_write'] },
  { key: 'groupExec', tools: ['bash_exec'] },
]

const LEVEL_KEYS: Record<string, string> = {
  L1Confirm: 'agentDetailSub.levelL1',
  L2Notify: 'agentDetailSub.levelL2',
  L3Autonomous: 'agentDetailSub.levelL3',
}

function AutonomyTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const [config, setConfig] = useState<AutonomyConfigData | null>(null)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    (async () => {
      try {
        const result = await invoke<AutonomyConfigData>('get_autonomy_config', { agentId })
        setConfig(result)
      } catch (e) { console.error(e) }
    })()
  }, [agentId])

  const handleLevelChange = async (tool: string, level: string) => {
    if (!config) return
    const newConfig = { ...config, overrides: { ...config.overrides, [tool]: level } }
    setConfig(newConfig)
    setSaving(true)
    try {
      await invoke('update_autonomy_config', { agentId, autonomyConfig: newConfig })
    } catch (e) { console.error(e); toast.error(t('common.error') + ': ' + String(e)) }
    finally { setSaving(false) }
  }

  if (!config) return <div style={{ padding: 20, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: 20, maxWidth: 700 }}>
      {/* 头部 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8 }}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
        </svg>
        <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>{t('agentDetailSub.autonomyTitle')}</h3>
        <span style={{ flex: 1 }} />
        {saving && <span style={{ fontSize: 11, color: 'var(--accent)', display: 'flex', alignItems: 'center', gap: 4 }}>
          <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: 'var(--accent)', animation: 'glow-pulse 1s infinite' }} />
          {t('common.saving')}
        </span>}
      </div>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 20px', lineHeight: 1.5 }}>
        {t('agentDetailSub.autonomyDesc')}
      </p>

      {/* 图例 */}
      <div style={{ display: 'flex', gap: 16, marginBottom: 20, padding: '10px 14px', borderRadius: 10, backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)' }}>
        {Object.entries(LEVEL_COLORS).map(([key, val]) => (
          <div key={key} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11 }}>
            <span style={{ width: 8, height: 8, borderRadius: '50%', backgroundColor: val.color }} />
            <span style={{ color: val.color, fontWeight: 500 }}>{t(LEVEL_KEYS[key])}</span>
          </div>
        ))}
      </div>

      {/* 分组卡片 */}
      {TOOL_GROUP_TOOLS.map((group) => (
        <div key={group.key} style={{
          marginBottom: 14, borderRadius: 12, border: '1px solid var(--border-subtle)',
          backgroundColor: 'var(--bg-elevated)', overflow: 'hidden',
        }}>
          {/* 分组头 */}
          <div style={{
            padding: '10px 16px', display: 'flex', alignItems: 'center', gap: 8,
            borderBottom: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
          }}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d={GROUP_ICONS[group.key] || 'M12 2L2 7l10 5 10-5-10-5z'}/>
            </svg>
            <span style={{ fontSize: 13, fontWeight: 600 }}>{t(`agentDetailSub.${group.key}`)}</span>
            <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>({group.tools.length})</span>
          </div>

          {/* 工具行 */}
          {group.tools.map((tool, idx) => {
            const current = config.overrides[tool] || config.default_level || 'L1Confirm'
            const currentColor = LEVEL_COLORS[current]?.color || 'var(--text-muted)'
            return (
              <div key={tool} style={{
                display: 'flex', alignItems: 'center', padding: '10px 16px',
                borderBottom: idx < group.tools.length - 1 ? '1px solid var(--border-subtle)' : 'none',
                transition: 'background-color 0.1s',
              }}>
                {/* 当前等级指示灯 */}
                <span style={{ width: 6, height: 6, borderRadius: '50%', backgroundColor: currentColor, flexShrink: 0, marginRight: 10 }} />
                {/* 工具名 */}
                <span style={{ fontSize: 13, fontFamily: "'SF Mono', Monaco, monospace", flex: 1, color: 'var(--text-primary)' }}>{tool}</span>
                {/* 等级选择 */}
                <div style={{ display: 'flex', gap: 3, borderRadius: 8, padding: 2, backgroundColor: 'var(--bg-primary)' }}>
                  {Object.entries(LEVEL_COLORS).map(([key, val]) => {
                    const active = current === key
                    return (
                      <button
                        key={key}
                        onClick={() => handleLevelChange(tool, key)}
                        style={{
                          padding: '4px 10px', fontSize: 11, borderRadius: 6, cursor: 'pointer',
                          border: 'none',
                          backgroundColor: active ? val.bg : 'transparent',
                          color: active ? val.color : 'var(--text-muted)',
                          fontWeight: active ? 600 : 400,
                          transition: 'all 0.15s',
                        }}
                      >
                        {t(LEVEL_KEYS[key])}
                      </button>
                    )
                  })}
                </div>
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
}
