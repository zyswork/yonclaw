/**
 * 定时任务管理页面
 *
 * 展示任务列表、运行记录、调度器状态、创建新任务
 */

import { useState, useEffect, useCallback } from 'react'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import { useConfirm } from '../hooks/useConfirm'

// Tauri invoke
const invoke = (window as any).__TAURI__?.invoke || (async () => {})

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

  const loadJobs = useCallback(async () => {
    try {
      const data = await invoke('list_cron_jobs')
      setJobs(data || [])
    } catch (e) {
      console.error('加载任务失败:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const loadRuns = useCallback(async (jobId: string) => {
    try {
      const data = await invoke('list_cron_runs', { jobId, limit: 20 })
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

  if (loading) return <div style={{ padding: 20 }}>{t('common.loading')}</div>

  return (
    <div style={{ padding: '20px', maxWidth: 1200 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <h2 style={{ margin: 0, fontSize: 20 }}>{t('cron.title')}</h2>
        <button
          onClick={() => setShowCreate(!showCreate)}
          style={{ ...btnStyle, padding: '6px 16px', fontSize: 13, background: showCreate ? '#eee' : 'var(--accent)', color: showCreate ? '#333' : '#fff', border: 'none' }}
        >
          {showCreate ? t('common.cancel') : t('cron.btnCreate')}
        </button>
      </div>

      {/* 创建表单 */}
      {showCreate && (
        <div style={{ border: '1px solid var(--border-subtle)', borderRadius: 8, padding: 16, marginBottom: 20, background: '#fafafa' }}>
          {/* 基础字段 */}
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 12 }}>
            <label style={labelStyle}>
              {t('cron.fieldName')}
              <input value={form.name} onChange={e => updateForm({ name: e.target.value })}
                style={inputStyle} placeholder={t('cronExtra.namePlaceholder')} />
            </label>
            <label style={labelStyle}>
              {t('cron.fieldType')}
              <select value={form.jobType} onChange={e => updateForm({ jobType: e.target.value as CreateForm['jobType'] })} style={inputStyle}>
                <option value="agent">{t('cron.typeAgent')}</option>
                <option value="shell">{t('cron.typeShell')}</option>
                <option value="mcp_tool">MCP Tool</option>
              </select>
            </label>
          </div>

          {/* 调度方式 */}
          <div style={{ marginBottom: 12 }}>
            <label style={{ ...labelStyle, marginBottom: 4 }}>{t('cron.fieldSchedule')}</label>
            <div style={{ display: 'flex', gap: 12, marginBottom: 8 }}>
              {(['cron', 'every', 'at'] as const).map(k => (
                <label key={k} style={{ display: 'flex', alignItems: 'center', gap: 4, cursor: 'pointer' }}>
                  <input type="radio" name="scheduleKind" checked={form.scheduleKind === k}
                    onChange={() => updateForm({ scheduleKind: k })} />
                  {{ cron: t('cron.scheduleCron'), every: t('cron.scheduleEvery'), at: t('cron.scheduleAt') }[k]}
                </label>
              ))}
            </div>
            {form.scheduleKind === 'cron' && (
              <div style={{ display: 'grid', gridTemplateColumns: '2fr 1fr', gap: 12 }}>
                <input value={form.cronExpr} onChange={e => updateForm({ cronExpr: e.target.value })}
                  style={inputStyle} placeholder="0 9 * * * (min hr day mon wk)" />
                <select value={form.cronTz} onChange={e => updateForm({ cronTz: e.target.value })} style={inputStyle}>
                  <option value="Asia/Shanghai">Asia/Shanghai</option>
                  <option value="UTC">UTC</option>
                  <option value="America/New_York">America/New_York</option>
                  <option value="Europe/London">Europe/London</option>
                </select>
              </div>
            )}
            {form.scheduleKind === 'every' && (
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span>{t('cronExtra.everyLabel')}</span>
                <input type="number" min={60} value={form.everySecs}
                  onChange={e => updateForm({ everySecs: parseInt(e.target.value) || 60 })}
                  style={{ ...inputStyle, width: 120 }} />
                <span>{t('cronExtra.secondsLabel')}</span>
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
                  <select value={form.sessionStrategy} onChange={e => updateForm({ sessionStrategy: e.target.value as CreateForm['sessionStrategy'] })} style={inputStyle}>
                    <option value="new">{t('cronExtra.sessionNew')}</option>
                    <option value="reuse">{t('cronExtra.sessionReuse')}</option>
                  </select>
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
                    style={{ ...inputStyle, minHeight: 60, fontFamily: 'monospace' }} />
                </label>
              </div>
            )}
          </div>

          {/* 高级选项 */}
          <div style={{ marginBottom: 12 }}>
            <button onClick={() => setShowAdvanced(!showAdvanced)}
              style={{ ...btnStyle, border: 'none', background: 'none', color: 'var(--text-secondary)', padding: 0, fontSize: 13 }}>
              {showAdvanced ? '▼' : '▶'} {t('cron.advancedOptions')}
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
            style={{ ...btnStyle, padding: '8px 24px', fontSize: 14, background: 'var(--accent)', color: '#fff', border: 'none', opacity: creating ? 0.6 : 1 }}>
            {creating ? t('common.creating') : t('common.create')}
          </button>
        </div>
      )}

      {/* 任务列表 */}
      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 14 }}>
        <thead>
          <tr style={{ borderBottom: '2px solid #e0e0e0', textAlign: 'left' }}>
            <th style={thStyle}>{t('cron.columnStatus')}</th>
            <th style={thStyle}>{t('cron.columnName')}</th>
            <th style={thStyle}>{t('cron.columnType')}</th>
            <th style={thStyle}>{t('cron.columnSchedule')}</th>
            <th style={thStyle}>{t('cron.columnNextRun')}</th>
            <th style={thStyle}>{t('cron.columnFailures')}</th>
            <th style={thStyle}>{t('common.actions')}</th>
          </tr>
        </thead>
        <tbody>
          {jobs.length === 0 ? (
            <tr><td colSpan={7} style={{ padding: 20, textAlign: 'center', color: 'var(--text-muted)' }}>
              {t('cron.emptyJobs')}
            </td></tr>
          ) : jobs.map(job => (
            <tr
              key={job.id}
              onClick={() => setSelectedJob(job.id)}
              style={{
                borderBottom: '1px solid #eee',
                cursor: 'pointer',
                backgroundColor: selectedJob === job.id ? '#e8f0fe' : 'transparent',
              }}
            >
              <td style={tdStyle}>
                <span style={{
                  display: 'inline-block', width: 8, height: 8, borderRadius: '50%',
                  backgroundColor: job.enabled ? '#4caf50' : '#999',
                }} />
              </td>
              <td style={tdStyle}>{job.name}</td>
              <td style={tdStyle}>{job.jobType}</td>
              <td style={tdStyle}>{scheduleDesc(job.schedule, t)}</td>
              <td style={tdStyle}>{formatTime(job.nextRunAt)}</td>
              <td style={tdStyle}>
                {job.failStreak > 0 && (
                  <span style={{ color: '#f44336' }}>{job.failStreak}</span>
                )}
              </td>
              <td style={tdStyle}>
                <button onClick={(e) => { e.stopPropagation(); handleToggle(job) }}
                  style={btnStyle}>{job.enabled ? t('cron.actionPause') : t('cron.actionResume')}</button>
                <button onClick={(e) => { e.stopPropagation(); handleTrigger(job.id) }}
                  style={btnStyle}>{t('cron.actionTrigger')}</button>
                <button onClick={(e) => { e.stopPropagation(); handleDelete(job.id) }}
                  style={{ ...btnStyle, color: '#f44336' }}>{t('common.delete')}</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {/* 运行记录 */}
      {selectedJob && (
        <div style={{ marginTop: 24 }}>
          <h3 style={{ fontSize: 16, margin: '0 0 12px' }}>
            {t('cron.sectionRuns')} - {jobs.find(j => j.id === selectedJob)?.name}
          </h3>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
            <thead>
              <tr style={{ borderBottom: '2px solid #e0e0e0', textAlign: 'left' }}>
                <th style={thStyle}>{t('cronExtra.runStatus')}</th>
                <th style={thStyle}>{t('cronExtra.runTrigger')}</th>
                <th style={thStyle}>{t('cronExtra.runStart')}</th>
                <th style={thStyle}>{t('cronExtra.runFinish')}</th>
                <th style={thStyle}>{t('cronExtra.runOutput')}</th>
              </tr>
            </thead>
            <tbody>
              {runs.length === 0 ? (
                <tr><td colSpan={5} style={{ padding: 12, color: 'var(--text-muted)' }}>{t('common.noRecords')}</td></tr>
              ) : runs.map(run => (
                <tr key={run.id} style={{ borderBottom: '1px solid #eee' }}>
                  <td style={tdStyle}>
                    <span style={{ color: statusColor(run.status) }}>{run.status}</span>
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
      )}
    </div>
  )
}

const thStyle: React.CSSProperties = { padding: '8px 12px', fontWeight: 600 }
const tdStyle: React.CSSProperties = { padding: '8px 12px' }
const btnStyle: React.CSSProperties = {
  padding: '4px 8px', marginRight: 4, border: '1px solid var(--border-subtle)',
  borderRadius: 4, background: '#fff', cursor: 'pointer', fontSize: 12,
}
const labelStyle: React.CSSProperties = {
  display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13, color: 'var(--text-secondary)',
}
const inputStyle: React.CSSProperties = {
  padding: '6px 10px', border: '1px solid #ccc', borderRadius: 4, fontSize: 13,
  fontFamily: 'inherit', width: '100%', boxSizing: 'border-box',
}

function statusColor(status: string): string {
  switch (status) {
    case 'success': return '#4caf50'
    case 'failed': return '#f44336'
    case 'timeout': return '#ff9800'
    case 'running': return '#2196f3'
    case 'cancelled': return '#999'
    default: return '#333'
  }
}
