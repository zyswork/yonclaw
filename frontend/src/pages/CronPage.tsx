/**
 * 定时任务管理页面 — 深色毛玻璃卡片式布局
 *
 * 展示任务列表、运行记录、调度器状态、创建新任务
 */

import { useState, useEffect, useCallback, type CSSProperties } from 'react'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import { useConfirm } from '../hooks/useConfirm'

// Tauri invoke - 使用与其他页面一致的导入方式
import { invoke } from '@tauri-apps/api/tauri'
import Select from '../components/Select'

interface CronJob {
  id: string
  name: string
  jobType: string
  schedule: { kind: string; expr?: string; secs?: number; ts?: number; tz?: string }
  enabled: boolean
  failStreak: number
  runsToday: number
  nextRunAt: number | null
  lastRunAt: number | null
  createdAt: number
}

interface CronRun {
  id: string
  jobId: string
  status: string
  triggerSource: string
  startedAt: number | null
  finishedAt: number | null
  output: string | null
  error: string | null
}

interface CreateForm {
  name: string
  jobType: 'agent' | 'shell' | 'mcp_tool'
  scheduleKind: 'cron' | 'every' | 'at'
  cronExpr: string
  cronTz: string
  everySecs: number
  atDatetime: string
  // agent
  prompt: string
  sessionStrategy: 'new' | 'reuse'
  // shell
  command: string
  // mcp_tool
  serverName: string
  toolName: string
  toolArgs: string
  // 高级
  timeoutSecs: number
  maxConcurrent: number
  cooldownSecs: number
  maxDailyRuns: string
  maxConsecutiveFailures: number
}

const defaultForm: CreateForm = {
  name: '',
  jobType: 'agent',
  scheduleKind: 'cron',
  cronExpr: '0 9 * * *',
  cronTz: 'Asia/Shanghai',
  everySecs: 3600,
  atDatetime: '',
  prompt: '',
  sessionStrategy: 'new',
  command: '',
  serverName: '',
  toolName: '',
  toolArgs: '{}',
  timeoutSecs: 300,
  maxConcurrent: 1,
  cooldownSecs: 0,
  maxDailyRuns: '',
  maxConsecutiveFailures: 5,
}

function formatTime(ts: number | null): string {
  if (!ts) return '-'
  return new Date(ts * 1000).toLocaleString()
}

function scheduleDesc(s: CronJob['schedule'], t: (key: string) => string): string {
  if (s.kind === 'cron') return `cron: ${s.expr}`
  if (s.kind === 'every') return `${t('cronExtra.everyLabel')} ${s.secs}s`
  if (s.kind === 'at') return `${t('cronExtra.atLabel')}: ${formatTime(s.ts ?? null)}`
  return t('common.unknown')
}

/* ─── SVG 图标 ─────────────────────────────── */

function SvgIcon({ name, size = 20, color }: { name: string; size?: number; color?: string }) {
  const c = color || 'currentColor'
  const props = { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: c, strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const }

  switch (name) {
    case 'clock':
      return <svg {...props}><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
    case 'play':
      return <svg {...props}><polygon points="5 3 19 12 5 21 5 3" fill={c} stroke="none"/></svg>
    case 'pause':
      return <svg {...props}><rect x="6" y="4" width="4" height="16" rx="1" fill={c} stroke="none"/><rect x="14" y="4" width="4" height="16" rx="1" fill={c} stroke="none"/></svg>
    case 'trigger':
      return <svg {...props}><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
    case 'trash':
      return <svg {...props}><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
    case 'plus':
      return <svg {...props}><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
    case 'search':
      return <svg {...props}><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
    case 'calendar':
      return <svg {...props}><rect x="3" y="4" width="18" height="18" rx="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
    case 'chevron-down':
      return <svg {...props}><polyline points="6 9 12 15 18 9"/></svg>
    case 'chevron-right':
      return <svg {...props}><polyline points="9 6 15 12 9 18"/></svg>
    case 'empty':
      return (
        <svg width={size} height={size} viewBox="0 0 64 64" fill="none">
          <circle cx="32" cy="32" r="28" stroke="var(--border-default)" strokeWidth="2" strokeDasharray="4 4"/>
          <circle cx="32" cy="32" r="12" stroke="var(--text-muted)" strokeWidth="1.5"/>
          <polyline points="32 24 32 32 38 35" stroke="var(--text-muted)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      )
    default:
      return <svg {...props}><circle cx="12" cy="12" r="10"/></svg>
  }
}

/* ─── 样式常量 ─────────────────────────────── */

const CARD_STYLE: CSSProperties = {
  background: 'var(--bg-elevated)',
  border: '1px solid var(--border-subtle)',
  borderRadius: 12,
  backdropFilter: 'blur(var(--glass-blur))',
  transition: 'transform 0.2s ease, box-shadow 0.2s ease',
  boxShadow: '0 2px 8px rgba(0,0,0,0.15)',
}

/* ─── 主组件 ─────────────────────────────── */

export default function CronPage() {
  const { t } = useI18n()
  const confirm = useConfirm()
  const [jobs, setJobs] = useState<CronJob[]>([])
  const [selectedJob, setSelectedJob] = useState<string | null>(null)
  const [runs, setRuns] = useState<CronRun[]>([])
  const [loading, setLoading] = useState(true)
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState<CreateForm>({ ...defaultForm })
  const [creating, setCreating] = useState(false)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [hoveredCard, setHoveredCard] = useState<string | null>(null)

  const loadJobs = useCallback(async () => {
    try {
      const data = await invoke<CronJob[]>('list_cron_jobs')
      setJobs(data || [])
    } catch (e) {
      console.error('加载任务失败:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const loadRuns = useCallback(async (jobId: string) => {
    try {
      const data = await invoke<CronRun[]>('list_cron_runs', { jobId, limit: 20 })
      setRuns(data || [])
    } catch (e) {
      console.error('加载运行记录失败:', e)
    }
  }, [])

  useEffect(() => { loadJobs() }, [loadJobs])

  useEffect(() => {
    if (selectedJob) loadRuns(selectedJob)
  }, [selectedJob, loadRuns])

  // 监听 cron-run-complete 事件刷新
  useEffect(() => {
    const listen = (window as any).__TAURI__?.event?.listen
    if (!listen) return
    const unlisten = listen('cron-run-complete', () => {
      loadJobs()
      if (selectedJob) loadRuns(selectedJob)
    })
    return () => { unlisten?.then((fn: () => void) => fn()) }
  }, [loadJobs, loadRuns, selectedJob])

  const handleToggle = async (job: CronJob) => {
    try {
      if (job.enabled) {
        await invoke('pause_cron_job', { jobId: job.id })
      } else {
        await invoke('resume_cron_job', { jobId: job.id })
      }
      loadJobs()
    } catch (e) {
      toast.error(t('cronExtra.operationFailed') + ': ' + e)
    }
  }

  const handleTrigger = async (jobId: string) => {
    try {
      await invoke('trigger_cron_job', { jobId })
      loadJobs()
    } catch (e) {
      toast.error(t('cronExtra.triggerFailed') + ': ' + e)
    }
  }

  const handleDelete = async (jobId: string) => {
    if (!await confirm(t('cron.confirmDelete'))) return
    try {
      await invoke('delete_cron_job', { jobId })
      if (selectedJob === jobId) setSelectedJob(null)
      loadJobs()
    } catch (e) {
      toast.error(t('cronExtra.deleteFailed') + ': ' + e)
    }
  }

  const handleCreate = async () => {
    if (!form.name.trim()) { toast.info(t('cronExtra.validationName')); return }

    // 构建 schedule
    let schedule: { kind: string; expr?: string; tz?: string; secs?: number; ts?: number }
    if (form.scheduleKind === 'cron') {
      schedule = { kind: 'cron', expr: form.cronExpr, tz: form.cronTz }
    } else if (form.scheduleKind === 'every') {
      schedule = { kind: 'every', secs: form.everySecs }
    } else {
      const ts = form.atDatetime ? Math.floor(new Date(form.atDatetime).getTime() / 1000) : 0
      if (!ts) { toast.info(t('cronExtra.validationTime')); return }
      schedule = { kind: 'at', ts }
    }

    // 构建 actionPayload
    let actionPayload: { type: string; prompt?: string; sessionStrategy?: string; command?: string; serverName?: string; toolName?: string; args?: Record<string, unknown> }
    if (form.jobType === 'agent') {
      if (!form.prompt.trim()) { toast.info(t('cronExtra.validationPrompt')); return }
      actionPayload = { type: 'agent', prompt: form.prompt, sessionStrategy: form.sessionStrategy }
    } else if (form.jobType === 'shell') {
      if (!form.command.trim()) { toast.info(t('cronExtra.validationCommand')); return }
      actionPayload = { type: 'shell', command: form.command }
    } else {
      if (!form.serverName.trim() || !form.toolName.trim()) { toast.info(t('cronExtra.validationMcp')); return }
      let args = {}
      try { args = JSON.parse(form.toolArgs) } catch { toast.info(t('cronExtra.validationJson')); return }
      actionPayload = { type: 'mcp_tool', serverName: form.serverName, toolName: form.toolName, args }
    }

    const payload = {
      name: form.name,
      agentId: null,
      jobType: form.jobType,
      schedule,
      actionPayload,
      timeoutSecs: form.timeoutSecs,
      guardrails: {
        maxConcurrent: form.maxConcurrent,
        cooldownSecs: form.cooldownSecs,
        maxDailyRuns: form.maxDailyRuns ? parseInt(form.maxDailyRuns) : null,
        maxConsecutiveFailures: form.maxConsecutiveFailures,
      },
      retry: { maxAttempts: 0, baseDelayMs: 2000, backoffFactor: 2.0 },
      misfirePolicy: 'catch_up',
      catchUpLimit: 3,
      deleteAfterRun: false,
    }

    setCreating(true)
    try {
      await invoke('create_cron_job', { payload })
      await loadJobs()
      setShowCreate(false)
      setForm({ ...defaultForm })
      setShowAdvanced(false)
    } catch (e) {
      toast.error(t('cronExtra.createFailed') + ': ' + e)
    } finally {
      setCreating(false)
    }
  }

  const updateForm = (patch: Partial<CreateForm>) => setForm(f => ({ ...f, ...patch }))

  // 搜索过滤
  const q = searchQuery.toLowerCase().trim()
  const filteredJobs = q
    ? jobs.filter(j => j.name.toLowerCase().includes(q) || j.jobType.toLowerCase().includes(q))
    : jobs

  if (loading) return (
    <div style={{ padding: 40, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
      {t('common.loading')}
    </div>
  )

  return (
    <div style={{ padding: '24px 32px', maxWidth: 1000 }}>
      {/* 标题区 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 24 }}>
        <div style={{
          width: 42, height: 42, borderRadius: 12,
          background: 'var(--accent-gradient)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          boxShadow: '0 4px 16px rgba(16, 185, 129, 0.3)',
        }}>
          <SvgIcon name="clock" size={22} color="#fff" />
        </div>
        <div>
          <h1 style={{
            margin: 0, fontSize: 22, fontWeight: 700,
            background: 'var(--accent-gradient)',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
          }}>
            {t('cron.title')}
          </h1>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
            {jobs.length} {jobs.length === 1 ? 'task' : 'tasks'}
          </span>
        </div>
        <span style={{ flex: 1 }} />
        <button
          onClick={() => setShowCreate(!showCreate)}
          style={{
            display: 'flex', alignItems: 'center', gap: 6,
            padding: '8px 18px', borderRadius: 10, fontSize: 13, fontWeight: 600,
            cursor: 'pointer', border: 'none', transition: 'all 0.2s ease',
            background: showCreate ? 'var(--bg-glass)' : 'var(--accent-gradient)',
            color: showCreate ? 'var(--text-primary)' : '#fff',
            boxShadow: showCreate ? 'none' : '0 4px 12px rgba(16, 185, 129, 0.3)',
          }}
        >
          <SvgIcon name="plus" size={16} color={showCreate ? 'var(--text-primary)' : '#fff'} />
          {showCreate ? t('common.cancel') : t('cron.btnCreate')}
        </button>
      </div>

      {/* 搜索框 */}
      <div style={{ position: 'relative', marginBottom: 20 }}>
        <div style={{
          position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)',
          color: 'var(--text-muted)', display: 'flex', alignItems: 'center',
        }}>
          <SvgIcon name="search" size={16} />
        </div>
        <input
          type="text"
          placeholder={t('cron.searchPlaceholder') || 'Search tasks...'}
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          style={{
            width: '100%', padding: '10px 14px 10px 40px',
            borderRadius: 10, border: '1px solid var(--border-subtle)',
            fontSize: 13, backgroundColor: 'var(--bg-elevated)',
            color: 'var(--text-primary)', boxSizing: 'border-box',
            outline: 'none', transition: 'border-color 0.2s ease',
          }}
        />
      </div>

      {/* 创建表单 */}
      {showCreate && (
        <div style={{ ...CARD_STYLE, padding: 20, marginBottom: 24 }}>
          {/* 基础字段 */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 12 }}>
            <label style={labelStyle}>
              {t('cron.fieldName')}
              <input value={form.name} onChange={e => updateForm({ name: e.target.value })}
                style={inputStyle} placeholder={t('cronExtra.namePlaceholder')} />
            </label>
            <label style={labelStyle}>
              {t('cron.fieldType')}
              <Select value={form.jobType} onChange={v => updateForm({ jobType: v as CreateForm['jobType'] })}
                options={[
                  { value: 'agent', label: t('cron.typeAgent') },
                  { value: 'shell', label: t('cron.typeShell') },
                  { value: 'mcp_tool', label: 'MCP Tool' },
                ]}
                style={{ width: '100%' }} />
            </label>
          </div>

          {/* 调度方式 */}
          <div style={{ marginBottom: 12 }}>
            <label style={{ ...labelStyle, marginBottom: 4 }}>{t('cron.fieldSchedule')}</label>
            <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
              {(['cron', 'every', 'at'] as const).map(k => (
                <button
                  key={k}
                  onClick={() => updateForm({ scheduleKind: k })}
                  style={{
                    padding: '5px 14px', borderRadius: 20, fontSize: 12, cursor: 'pointer',
                    border: 'none', fontWeight: form.scheduleKind === k ? 600 : 400,
                    backgroundColor: form.scheduleKind === k ? 'var(--accent)' : 'var(--bg-glass)',
                    color: form.scheduleKind === k ? '#fff' : 'var(--text-secondary)',
                    transition: 'all 0.15s ease',
                  }}
                >
                  {{ cron: t('cron.scheduleCron'), every: t('cron.scheduleEvery'), at: t('cron.scheduleAt') }[k]}
                </button>
              ))}
            </div>
            {form.scheduleKind === 'cron' && (
              <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 12 }}>
                <input value={form.cronExpr} onChange={e => updateForm({ cronExpr: e.target.value })}
                  style={{ ...inputStyle, fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace' }} placeholder="0 9 * * * (min hr day mon wk)" />
                <Select value={form.cronTz} onChange={v => updateForm({ cronTz: v })}
                  options={[
                    { value: 'Asia/Shanghai', label: 'Asia/Shanghai' },
                    { value: 'UTC', label: 'UTC' },
                    { value: 'America/New_York', label: 'America/New_York' },
                    { value: 'Europe/London', label: 'Europe/London' },
                  ]}
                  style={{ width: '100%' }} />
              </div>
            )}
            {form.scheduleKind === 'every' && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{t('cronExtra.everyLabel')}</span>
                <input type="number" min={60} value={form.everySecs}
                  onChange={e => updateForm({ everySecs: parseInt(e.target.value) || 60 })}
                  style={{ ...inputStyle, width: 120 }} />
                <span style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{t('cronExtra.secondsLabel')}</span>
              </div>
            )}
            {form.scheduleKind === 'at' && (
              <input type="datetime-local" value={form.atDatetime}
                onChange={e => updateForm({ atDatetime: e.target.value })} style={inputStyle} />
            )}
          </div>

          {/* 动作配置 */}
          <div style={{ marginBottom: 12 }}>
            {form.jobType === 'agent' && (
              <>
                <label style={labelStyle}>
                  Prompt
                  <textarea value={form.prompt} onChange={e => updateForm({ prompt: e.target.value })}
                    style={{ ...inputStyle, minHeight: 80, resize: 'vertical' }} placeholder={t('cron.promptPlaceholder')} />
                </label>
                <label style={{ ...labelStyle, marginTop: 8 }}>
                  {t('cronExtra.sessionStrategy')}
                  <Select value={form.sessionStrategy} onChange={v => updateForm({ sessionStrategy: v as CreateForm['sessionStrategy'] })}
                    options={[
                      { value: 'new', label: t('cronExtra.sessionNew') },
                      { value: 'reuse', label: t('cronExtra.sessionReuse') },
                    ]}
                    style={{ width: '100%' }} />
                </label>
              </>
            )}
            {form.jobType === 'shell' && (
              <label style={labelStyle}>
                {t('cronExtra.shellCommand')}
                <input value={form.command} onChange={e => updateForm({ command: e.target.value })}
                  style={inputStyle} placeholder="e.g. echo hello" />
              </label>
            )}
            {form.jobType === 'mcp_tool' && (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                <label style={labelStyle}>
                  {t('cronExtra.mcpServer')}
                  <input value={form.serverName} onChange={e => updateForm({ serverName: e.target.value })}
                    style={inputStyle} placeholder="server_name" />
                </label>
                <label style={labelStyle}>
                  {t('cronExtra.toolName')}
                  <input value={form.toolName} onChange={e => updateForm({ toolName: e.target.value })}
                    style={inputStyle} placeholder="tool_name" />
                </label>
                <label style={{ ...labelStyle, gridColumn: '1 / -1' }}>
                  {t('cronExtra.toolArgs')}
                  <textarea value={form.toolArgs} onChange={e => updateForm({ toolArgs: e.target.value })}
                    style={{ ...inputStyle, minHeight: 60, fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace' }} />
                </label>
              </div>
            )}
          </div>

          {/* 高级选项 */}
          <div style={{ marginBottom: 16 }}>
            <button onClick={() => setShowAdvanced(!showAdvanced)}
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                border: 'none', background: 'none', color: 'var(--text-secondary)',
                padding: 0, fontSize: 13, cursor: 'pointer',
              }}>
              <SvgIcon name={showAdvanced ? 'chevron-down' : 'chevron-right'} size={14} />
              {t('cron.advancedOptions')}
            </button>
            {showAdvanced && (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 12, marginTop: 8 }}>
                <label style={labelStyle}>
                  {t('cron.fieldTimeout')}
                  <input type="number" value={form.timeoutSecs}
                    onChange={e => updateForm({ timeoutSecs: parseInt(e.target.value) || 300 })} style={inputStyle} />
                </label>
                <label style={labelStyle}>
                  {t('cron.fieldMaxConcurrent')}
                  <input type="number" min={1} value={form.maxConcurrent}
                    onChange={e => updateForm({ maxConcurrent: parseInt(e.target.value) || 1 })} style={inputStyle} />
                </label>
                <label style={labelStyle}>
                  {t('cron.fieldCooldown')}
                  <input type="number" min={0} value={form.cooldownSecs}
                    onChange={e => updateForm({ cooldownSecs: parseInt(e.target.value) || 0 })} style={inputStyle} />
                </label>
                <label style={labelStyle}>
                  {t('cron.fieldMaxDaily')}
                  <input type="number" min={0} value={form.maxDailyRuns}
                    onChange={e => updateForm({ maxDailyRuns: e.target.value })} style={inputStyle} placeholder={t('cron.unlimited')} />
                </label>
                <label style={labelStyle}>
                  {t('cronExtra.maxConsecutiveFailures')}
                  <input type="number" min={1} value={form.maxConsecutiveFailures}
                    onChange={e => updateForm({ maxConsecutiveFailures: parseInt(e.target.value) || 5 })} style={inputStyle} />
                </label>
              </div>
            )}
          </div>

          <button onClick={handleCreate} disabled={creating}
            style={{
              display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6,
              padding: '10px 28px', fontSize: 14, fontWeight: 600,
              background: 'var(--accent-gradient)', color: '#fff', border: 'none',
              borderRadius: 10, cursor: creating ? 'not-allowed' : 'pointer',
              opacity: creating ? 0.6 : 1, transition: 'opacity 0.2s ease',
              boxShadow: '0 4px 12px rgba(16, 185, 129, 0.3)',
            }}>
            {creating ? t('common.creating') : t('common.create')}
          </button>
        </div>
      )}

      {/* 任务卡片列表 */}
      {filteredJobs.length === 0 ? (
        <div style={{
          ...CARD_STYLE, padding: '48px 24px',
          display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 16,
        }}>
          <SvgIcon name="empty" size={64} />
          <div style={{ color: 'var(--text-muted)', fontSize: 14 }}>
            {searchQuery ? t('common.noResults') || 'No matching tasks' : t('cron.emptyJobs')}
          </div>
          {!searchQuery && (
            <button
              onClick={() => setShowCreate(true)}
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '8px 18px', borderRadius: 10, fontSize: 13, fontWeight: 500,
                border: '1px solid var(--border-subtle)', background: 'var(--bg-glass)',
                color: 'var(--text-accent)', cursor: 'pointer',
              }}
            >
              <SvgIcon name="plus" size={14} color="var(--text-accent)" />
              {t('cron.btnCreate')}
            </button>
          )}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {filteredJobs.map(job => {
            const isSelected = selectedJob === job.id
            const isHovered = hoveredCard === job.id
            return (
              <div
                key={job.id}
                onClick={() => setSelectedJob(isSelected ? null : job.id)}
                onMouseEnter={() => setHoveredCard(job.id)}
                onMouseLeave={() => setHoveredCard(null)}
                style={{
                  ...CARD_STYLE,
                  padding: '16px 20px',
                  cursor: 'pointer',
                  display: 'flex', alignItems: 'center', gap: 16,
                  transform: isHovered ? 'translateY(-1px)' : 'none',
                  boxShadow: isHovered ? '0 8px 24px rgba(0,0,0,0.25)' : '0 2px 8px rgba(0,0,0,0.15)',
                  borderColor: isSelected ? 'var(--border-accent)' : 'var(--border-subtle)',
                  background: isSelected ? 'var(--bg-glass-active)' : 'var(--bg-elevated)',
                }}
              >
                {/* 状态指示灯 */}
                <div style={{
                  width: 10, height: 10, borderRadius: '50%', flexShrink: 0,
                  backgroundColor: job.enabled ? '#22c55e' : 'var(--text-muted)',
                  boxShadow: job.enabled ? '0 0 8px rgba(34, 197, 94, 0.5)' : 'none',
                  transition: 'all 0.2s ease',
                }} />

                {/* 任务信息 */}
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
                    <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)' }}>
                      {job.name}
                    </span>
                    <span style={{
                      fontSize: 11, fontFamily: 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace',
                      padding: '2px 8px', borderRadius: 6,
                      backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                      border: '1px solid var(--border-subtle)',
                    }}>
                      {scheduleDesc(job.schedule, t)}
                    </span>
                    <span style={{
                      fontSize: 10, padding: '2px 8px', borderRadius: 6,
                      backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)',
                    }}>
                      {job.jobType}
                    </span>
                    {job.failStreak > 0 && (
                      <span style={{
                        fontSize: 10, padding: '2px 8px', borderRadius: 6,
                        backgroundColor: 'var(--error-bg)', color: 'var(--error)',
                        fontWeight: 600,
                      }}>
                        {job.failStreak} fails
                      </span>
                    )}
                  </div>
                  <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>
                    <SvgIcon name="calendar" size={12} color="var(--text-muted)" />
                    <span style={{ marginLeft: 4 }}>
                      {t('cron.columnNextRun')}: {formatTime(job.nextRunAt)}
                    </span>
                  </div>
                </div>

                {/* 操作按钮 */}
                <div style={{ display: 'flex', gap: 6, flexShrink: 0 }} onClick={e => e.stopPropagation()}>
                  <button
                    onClick={() => handleTrigger(job.id)}
                    title={t('cron.actionTrigger')}
                    style={iconBtnStyle}
                  >
                    <SvgIcon name="trigger" size={15} color="var(--text-accent)" />
                  </button>
                  <button
                    onClick={() => handleToggle(job)}
                    title={job.enabled ? t('cron.actionPause') : t('cron.actionResume')}
                    style={iconBtnStyle}
                  >
                    <SvgIcon name={job.enabled ? 'pause' : 'play'} size={15} color={job.enabled ? 'var(--warning)' : 'var(--success)'} />
                  </button>
                  <button
                    onClick={() => handleDelete(job.id)}
                    title={t('common.delete')}
                    style={iconBtnStyle}
                  >
                    <SvgIcon name="trash" size={15} color="var(--error)" />
                  </button>
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* 运行记录 */}
      {selectedJob && (
        <div style={{ marginTop: 24 }}>
          <h3 style={{ fontSize: 16, fontWeight: 600, margin: '0 0 12px', color: 'var(--text-primary)' }}>
            {t('cron.sectionRuns')} - {jobs.find(j => j.id === selectedJob)?.name}
          </h3>
          <div style={{ ...CARD_STYLE, overflow: 'hidden' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
              <thead>
                <tr style={{ borderBottom: '1px solid var(--border-subtle)' }}>
                  <th style={thStyle}>{t('cronExtra.runStatus')}</th>
                  <th style={thStyle}>{t('cronExtra.runTrigger')}</th>
                  <th style={thStyle}>{t('cronExtra.runStart')}</th>
                  <th style={thStyle}>{t('cronExtra.runFinish')}</th>
                  <th style={thStyle}>{t('cronExtra.runOutput')}</th>
                </tr>
              </thead>
              <tbody>
                {runs.length === 0 ? (
                  <tr><td colSpan={5} style={{ padding: 20, textAlign: 'center', color: 'var(--text-muted)' }}>{t('common.noRecords')}</td></tr>
                ) : runs.map(run => (
                  <tr key={run.id} style={{ borderBottom: '1px solid var(--border-subtle)' }}>
                    <td style={tdStyle}>
                      <span style={{
                        fontSize: 11, padding: '2px 8px', borderRadius: 6,
                        backgroundColor: statusBg(run.status), color: statusColor(run.status),
                        fontWeight: 600,
                      }}>
                        {run.status}
                      </span>
                    </td>
                    <td style={tdStyle}>{run.triggerSource}</td>
                    <td style={tdStyle}>{formatTime(run.startedAt)}</td>
                    <td style={tdStyle}>{formatTime(run.finishedAt)}</td>
                    <td style={{ ...tdStyle, maxWidth: 300, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {run.error || run.output || '-'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}

/* ─── 样式常量 ─────────────────────────────── */

const thStyle: CSSProperties = { padding: '10px 14px', fontWeight: 600, textAlign: 'left', color: 'var(--text-secondary)', fontSize: 12 }
const tdStyle: CSSProperties = { padding: '10px 14px' }

const iconBtnStyle: CSSProperties = {
  width: 34, height: 34, borderRadius: 10,
  display: 'flex', alignItems: 'center', justifyContent: 'center',
  border: '1px solid var(--border-subtle)', background: 'var(--bg-glass)',
  cursor: 'pointer', transition: 'all 0.15s ease',
}

const labelStyle: CSSProperties = {
  display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13, color: 'var(--text-secondary)',
}
const inputStyle: CSSProperties = {
  padding: '8px 12px', border: '1px solid var(--border-subtle)', borderRadius: 10, fontSize: 13,
  fontFamily: 'inherit', width: '100%', boxSizing: 'border-box',
  backgroundColor: 'var(--bg-glass)', color: 'var(--text-primary)',
  outline: 'none', transition: 'border-color 0.2s ease',
}

function statusColor(status: string): string {
  switch (status) {
    case 'success': return '#22c55e'
    case 'failed': return '#ef4444'
    case 'timeout': return '#f59e0b'
    case 'running': return '#3b82f6'
    case 'cancelled': return 'var(--text-muted)'
    default: return 'var(--text-primary)'
  }
}

function statusBg(status: string): string {
  switch (status) {
    case 'success': return 'rgba(34, 197, 94, 0.1)'
    case 'failed': return 'rgba(239, 68, 68, 0.1)'
    case 'timeout': return 'rgba(245, 158, 11, 0.1)'
    case 'running': return 'rgba(59, 130, 246, 0.1)'
    default: return 'var(--bg-glass)'
  }
}
