/**
 * 灵魂文件 Tab
 *
 * 提供表单模式和原始模式两种编辑方式：
 * - 表单模式：解析 IDENTITY/SOUL/USER.md 为结构化字段
 * - 原始模式：直接编辑 markdown 文件内容
 */

import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface SoulFileTabProps {
  agentId: string
}

/** 后端返回的文件信息 */
interface SoulFileInfo {
  name: string
  exists: boolean
  size: number
}

/** 灵魂文件定义（含图标和描述） */
const SOUL_FILE_DEFS = [
  { name: 'IDENTITY.md', icon: 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 3c1.66 0 3 1.34 3 3s-1.34 3-3 3-3-1.34-3-3 1.34-3 3-3zm0 14.2c-2.5 0-4.71-1.28-6-3.22.03-1.99 4-3.08 6-3.08 1.99 0 5.97 1.09 6 3.08-1.29 1.94-3.5 3.22-6 3.22z', desc: 'Agent 身份标识：名称、角色、emoji' },
  { name: 'SOUL.md', icon: 'M12 2L15 8 22 9 17 14 18 21 12 18 6 21 7 14 2 9 9 8z', desc: 'Agent 核心人格：语气、风格、行为准则' },
  { name: 'AGENTS.md', icon: 'M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2M9 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75', desc: 'Agent 协作规则：与其他 Agent 交互方式' },
  { name: 'USER.md', icon: 'M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M12 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8', desc: 'User 画像：用户偏好、习惯（自动学习）' },
  { name: 'TOOLS.md', icon: 'M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z', desc: '工具配置：自定义工具参数和策略' },
  { name: 'MEMORY.md', icon: 'M9.5 2A2.5 2.5 0 0 1 12 4.5v15a2.5 2.5 0 0 1-4.96.44A2.5 2.5 0 0 1 4.08 16.9a3 3 0 0 1-.34-5.58 2.5 2.5 0 0 1 1.32-4.24A2.5 2.5 0 0 1 7.04 4.04 2.5 2.5 0 0 1 9.5 2M14.5 2A2.5 2.5 0 0 0 12 4.5v15a2.5 2.5 0 0 0 4.96.44 2.5 2.5 0 0 0 2.96-3.08 3 3 0 0 0 .34-5.58 2.5 2.5 0 0 0-1.32-4.24 2.5 2.5 0 0 0-1.98-3A2.5 2.5 0 0 0 14.5 2', desc: '记忆指令：长期记忆的存储和检索规则' },
  { name: 'BOOTSTRAP.md', icon: 'M13 2L3 14h9l-1 8 10-12h-9l1-8z', desc: '启动脚本：Agent 首次启动时执行的初始化' },
  { name: 'HEARTBEAT.md', icon: 'M22 12h-4l-3 9L9 3l-3 9H2', desc: '心跳任务：Agent 定期自动执行的任务' },
  { name: 'STANDING_ORDERS.md', icon: 'M9 11l3 3L22 4M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11', desc: '常驻指令：每次对话都会注入的规则' },
]
const ALL_SOUL_FILES = SOUL_FILE_DEFS.map(d => d.name)

/** 表单模式的字段结构 */
interface FormFields {
  name: string
  emoji: string
  type: string
  personality: string
  style: string
  values: string
  userName: string
  timezone: string
  language: string
  // 自定义内容（当文件格式不匹配表单结构时保留原始内容）
  customIdentity: string
  customSoul: string
  customUser: string
}

const DEFAULT_FORM: FormFields = {
  name: '', emoji: '', type: '',
  personality: '', style: '', values: '',
  userName: '', timezone: '', language: '',
  customIdentity: '', customSoul: '', customUser: '',
}

/** 从 markdown 内容解析键值对 */
function parseMdFields(content: string): Record<string, string> {
  const fields: Record<string, string> = {}
  const lines = content.split('\n')
  let currentKey = ''
  let currentValue: string[] = []

  for (const line of lines) {
    // 匹配 "## Key" 或 "**Key**: Value" 或 "- Key: Value"
    const headerMatch = line.match(/^##\s+(.+)/)
    const boldMatch = line.match(/^\*\*(.+?)\*\*:\s*(.*)/)
    const dashMatch = line.match(/^-\s+(.+?):\s*(.*)/)

    if (headerMatch) {
      if (currentKey) fields[currentKey] = currentValue.join('\n').trim()
      currentKey = headerMatch[1].trim().toLowerCase()
      currentValue = []
    } else if (boldMatch) {
      if (currentKey) fields[currentKey] = currentValue.join('\n').trim()
      currentKey = boldMatch[1].trim().toLowerCase()
      currentValue = [boldMatch[2]]
    } else if (dashMatch && !currentKey) {
      fields[dashMatch[1].trim().toLowerCase()] = dashMatch[2].trim()
    } else if (currentKey) {
      currentValue.push(line)
    }
  }
  if (currentKey) fields[currentKey] = currentValue.join('\n').trim()
  return fields
}

/** 将表单字段重建为 markdown */
function buildIdentityMd(f: FormFields): string {
  const lines = ['# Identity', '']
  if (f.name) lines.push(`**Name**: ${f.name}`)
  if (f.emoji) lines.push(`**Emoji**: ${f.emoji}`)
  if (f.type) lines.push(`**Type**: ${f.type}`)
  return lines.join('\n') + '\n'
}

function buildSoulMd(f: FormFields): string {
  const lines = ['# Soul', '']
  if (f.personality) lines.push(`## Personality\n${f.personality}\n`)
  if (f.style) lines.push(`## Style\n${f.style}\n`)
  if (f.values) lines.push(`## Values\n${f.values}\n`)
  return lines.join('\n') + '\n'
}

function buildUserMd(f: FormFields): string {
  const lines = ['# User', '']
  if (f.userName) lines.push(`**Name**: ${f.userName}`)
  if (f.timezone) lines.push(`**Timezone**: ${f.timezone}`)
  if (f.language) lines.push(`**Language**: ${f.language}`)
  return lines.join('\n') + '\n'
}

export default function SoulFileTab({ agentId }: SoulFileTabProps) {
  const { t } = useI18n()
  const [rawMode, setRawMode] = useState(false)
  const [form, setForm] = useState<FormFields>({ ...DEFAULT_FORM })
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState('')

  // 原始模式状态
  const [fileList, setFileList] = useState<string[]>([])
  const [selectedFile, setSelectedFile] = useState('')
  const [fileContent, setFileContent] = useState('')
  const [fileSizes, setFileSizes] = useState<Record<string, number>>({})

  /** 加载表单模式数据 */
  const loadFormData = useCallback(async () => {
    try {
      const fileInfos = await invoke<SoulFileInfo[]>('list_soul_files', { agentId })
      const existingFiles = (fileInfos || []).filter(f => f.exists).map(f => f.name)
      setFileList(existingFiles)

      // 加载 IDENTITY.md
      if (existingFiles.includes('IDENTITY.md')) {
        const content = await invoke<string>('read_soul_file', { agentId, fileName: 'IDENTITY.md' })
        const raw = content || ''
        const parsed = parseMdFields(raw)
        const hasFields = !!(parsed['name'] || parsed['emoji'] || parsed['type'])
        setForm(prev => ({
          ...prev,
          name: parsed['name'] || '',
          emoji: parsed['emoji'] || '',
          type: parsed['type'] || '',
          customIdentity: hasFields ? '' : raw,
        }))
      }

      // 加载 SOUL.md
      if (existingFiles.includes('SOUL.md')) {
        const content = await invoke<string>('read_soul_file', { agentId, fileName: 'SOUL.md' })
        const raw = content || ''
        const parsed = parseMdFields(raw)
        const hasFields = !!(parsed['personality'] || parsed['style'] || parsed['values'])
        setForm(prev => ({
          ...prev,
          personality: parsed['personality'] || '',
          style: parsed['style'] || '',
          values: parsed['values'] || '',
          customSoul: hasFields ? '' : raw,
        }))
      }

      // 加载 USER.md
      if (existingFiles.includes('USER.md')) {
        const content = await invoke<string>('read_soul_file', { agentId, fileName: 'USER.md' })
        const raw = content || ''
        const parsed = parseMdFields(raw)
        const hasFields = !!(parsed['name'] || parsed['timezone'] || parsed['language'])
        setForm(prev => ({
          ...prev,
          userName: parsed['name'] || '',
          timezone: parsed['timezone'] || '',
          language: parsed['language'] || '',
          customUser: hasFields ? '' : raw,
        }))
      }
    } catch (e) {
      console.error('加载灵魂文件失败:', e)
    }
  }, [agentId])

  /** 加载原始模式数据 */
  const loadRawData = useCallback(async () => {
    try {
      const fileInfos = await invoke<SoulFileInfo[]>('list_soul_files', { agentId })
      const existingFiles = (fileInfos || []).filter(f => f.exists).map(f => f.name)
      setFileList(existingFiles)

      // 直接用后端返回的 size，不再逐个读文件
      const sizes: Record<string, number> = {}
      for (const fi of fileInfos || []) {
        sizes[fi.name] = fi.exists ? fi.size : 0
      }
      setFileSizes(sizes)
    } catch (e) {
      console.error('加载文件列表失败:', e)
    }
  }, [agentId])

  useEffect(() => {
    if (rawMode) {
      loadRawData()
    } else {
      loadFormData()
    }
  }, [agentId, rawMode, loadFormData, loadRawData])

  /** 选择文件（原始模式） */
  const handleSelectFile = async (fileName: string) => {
    setSelectedFile(fileName)
    try {
      const content = await invoke<string>('read_soul_file', { agentId, fileName })
      setFileContent(content || '')
    } catch (e) {
      // 文件不存在，创建空内容
      console.debug('读取 soul 文件失败（可能不存在）:', e)
      setFileContent('')
    }
  }

  /** 保存表单模式 */
  const handleSaveForm = async () => {
    setSaving(true)
    setStatus('')
    try {
      // 自定义内容优先：如果有自定义内容则直接保存，否则按结构化格式生成
      const identityContent = form.customIdentity || buildIdentityMd(form)
      const soulContent = form.customSoul || buildSoulMd(form)
      const userContent = form.customUser || buildUserMd(form)

      await invoke('write_soul_file', { agentId, fileName: 'IDENTITY.md', content: identityContent })
      await invoke('write_soul_file', { agentId, fileName: 'SOUL.md', content: soulContent })
      await invoke('write_soul_file', { agentId, fileName: 'USER.md', content: userContent })
      setStatus(t('soulFile.saved'))
      setTimeout(() => setStatus(''), 2000)
    } catch (e) {
      setStatus(t('soulFile.saveFailed') + ': ' + String(e))
    } finally {
      setSaving(false)
    }
  }

  /** 保存原始文件 */
  const handleSaveRaw = async () => {
    if (!selectedFile) return
    setSaving(true)
    setStatus('')
    try {
      await invoke('write_soul_file', { agentId, fileName: selectedFile, content: fileContent })
      setStatus(t('soulFile.saved'))
      setFileSizes(prev => ({ ...prev, [selectedFile]: fileContent.length }))
      // 刷新文件列表（可能新建了文件）
      const files = await invoke<string[]>('list_soul_files', { agentId })
      setFileList(files || [])
      setTimeout(() => setStatus(''), 2000)
    } catch (e) {
      setStatus(t('soulFile.saveFailed') + ': ' + String(e))
    } finally {
      setSaving(false)
    }
  }

  const inputStyle = {
    width: '100%', padding: '6px 8px', border: '1px solid var(--border-subtle)',
    borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' as const,
  }
  const labelStyle = { fontSize: '12px', color: 'var(--text-secondary)', marginBottom: '3px', display: 'block' }
  const fieldGroup = { marginBottom: '10px' }

  // 表单模式
  if (!rawMode) {
    return (
      <div style={{ padding: '8px 0' }}>
        {/* 模式切换 */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: '10px' }}>
          <button
            onClick={() => setRawMode(true)}
            style={{
              fontSize: '11px', padding: '3px 8px', border: '1px solid var(--border-subtle)',
              borderRadius: '3px', background: 'var(--bg-glass)', cursor: 'pointer', color: 'var(--text-secondary)',
            }}
          >
            {t('soulFile.rawMode')}
          </button>
        </div>

        {/* IDENTITY 区域 */}
        <div style={{ marginBottom: '14px' }}>
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: 'var(--text-primary)' }}>{t('soulFile.sectionIdentity')}</div>
          {form.customIdentity ? (
            <div>
              <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
              <textarea
                style={{ ...inputStyle, minHeight: '80px', resize: 'vertical', fontFamily: 'monospace', fontSize: '11px' }}
                value={form.customIdentity}
                onChange={e => setForm(p => ({ ...p, customIdentity: e.target.value }))}
              />
            </div>
          ) : (
            <>
              <div style={fieldGroup}>
                <label style={labelStyle}>{t('soulFile.fieldName')}</label>
                <input style={inputStyle} value={form.name} onChange={e => setForm(p => ({ ...p, name: e.target.value }))} placeholder={t('soulFile.placeholderName')} />
              </div>
              <div style={{ display: 'flex', gap: '8px' }}>
                <div style={{ ...fieldGroup, flex: 1 }}>
                  <label style={labelStyle}>{t('soulFile.fieldEmoji')}</label>
                  <input style={inputStyle} value={form.emoji} onChange={e => setForm(p => ({ ...p, emoji: e.target.value }))} placeholder={t('soulFile.placeholderEmoji')} />
                </div>
                <div style={{ ...fieldGroup, flex: 1 }}>
                  <label style={labelStyle}>{t('soulFile.fieldType')}</label>
                  <input style={inputStyle} value={form.type} onChange={e => setForm(p => ({ ...p, type: e.target.value }))} placeholder={t('soulFile.placeholderType')} />
                </div>
              </div>
            </>
          )}
        </div>

        {/* SOUL 区域 */}
        <div style={{ marginBottom: '14px' }}>
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: 'var(--text-primary)' }}>{t('soulFile.sectionSoul')}</div>
          {form.customSoul ? (
            <div>
              <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
              <textarea
                style={{ ...inputStyle, minHeight: '120px', resize: 'vertical', fontFamily: 'monospace', fontSize: '11px' }}
                value={form.customSoul}
                onChange={e => setForm(p => ({ ...p, customSoul: e.target.value }))}
              />
            </div>
          ) : (
            <>
              <div style={fieldGroup}>
                <label style={labelStyle}>{t('soulFile.fieldPersonality')}</label>
                <textarea style={{ ...inputStyle, minHeight: '50px', resize: 'vertical' }} value={form.personality} onChange={e => setForm(p => ({ ...p, personality: e.target.value }))} placeholder={t('soulFile.placeholderPersonality')} />
              </div>
              <div style={fieldGroup}>
                <label style={labelStyle}>{t('soulFile.fieldStyle')}</label>
                <textarea style={{ ...inputStyle, minHeight: '40px', resize: 'vertical' }} value={form.style} onChange={e => setForm(p => ({ ...p, style: e.target.value }))} placeholder={t('soulFile.placeholderStyle')} />
              </div>
              <div style={fieldGroup}>
                <label style={labelStyle}>{t('soulFile.fieldValues')}</label>
                <textarea style={{ ...inputStyle, minHeight: '40px', resize: 'vertical' }} value={form.values} onChange={e => setForm(p => ({ ...p, values: e.target.value }))} placeholder={t('soulFile.placeholderValues')} />
              </div>
            </>
          )}
        </div>

        {/* USER 区域 */}
        <div style={{ marginBottom: '14px' }}>
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: 'var(--text-primary)' }}>{t('soulFile.sectionUser')}</div>
          {form.customUser ? (
            <div>
              <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
              <textarea
                style={{ ...inputStyle, minHeight: '80px', resize: 'vertical', fontFamily: 'monospace', fontSize: '11px' }}
                value={form.customUser}
                onChange={e => setForm(p => ({ ...p, customUser: e.target.value }))}
              />
            </div>
          ) : (
            <>
              <div style={fieldGroup}>
                <label style={labelStyle}>{t('soulFile.fieldUsername')}</label>
                <input style={inputStyle} value={form.userName} onChange={e => setForm(p => ({ ...p, userName: e.target.value }))} placeholder={t('soulFile.placeholderUsername')} />
              </div>
              <div style={{ display: 'flex', gap: '8px' }}>
                <div style={{ ...fieldGroup, flex: 1 }}>
                  <label style={labelStyle}>{t('soulFile.fieldTimezone')}</label>
                  <input style={inputStyle} value={form.timezone} onChange={e => setForm(p => ({ ...p, timezone: e.target.value }))} placeholder={t('soulFile.placeholderTimezone')} />
                </div>
                <div style={{ ...fieldGroup, flex: 1 }}>
                  <label style={labelStyle}>{t('soulFile.fieldLanguage')}</label>
                  <input style={inputStyle} value={form.language} onChange={e => setForm(p => ({ ...p, language: e.target.value }))} placeholder={t('soulFile.placeholderLanguage')} />
                </div>
              </div>
            </>
          )}
        </div>

        {/* 保存按钮 */}
        <button
          onClick={handleSaveForm}
          disabled={saving}
          style={{
            width: '100%', padding: '8px', backgroundColor: 'var(--accent)', color: 'white',
            border: 'none', borderRadius: '4px', cursor: saving ? 'not-allowed' : 'pointer',
            opacity: saving ? 0.6 : 1, fontSize: '13px',
          }}
        >
          {saving ? t('common.saving') : t('soulFile.saveSoulFiles')}
        </button>
        {status && (
          <div style={{ fontSize: '12px', color: status.startsWith(t('soulFile.saveFailed')) ? 'var(--error)' : 'var(--success)', marginTop: '6px', textAlign: 'center' }}>
            {status}
          </div>
        )}
      </div>
    )
  }

  // 原始模式
  return (
    <div style={{ padding: '8px 0', display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* 模式切换 */}
      <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: '10px' }}>
        <button
          onClick={() => { setRawMode(false); setSelectedFile('') }}
          style={{
            fontSize: '11px', padding: '3px 8px', border: '1px solid var(--border-subtle)',
            borderRadius: '3px', background: 'var(--bg-glass)', cursor: 'pointer', color: 'var(--text-secondary)',
          }}
        >
          {t('soulFile.formMode')}
        </button>
      </div>

      {/* 文件列表 */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 8, marginBottom: 12 }}>
        {SOUL_FILE_DEFS.map(def => {
          const exists = fileList.includes(def.name)
          const isSelected = selectedFile === def.name
          const size = fileSizes[def.name] || 0
          const isEmpty = exists && size === 0
          return (
            <div
              key={def.name}
              onClick={() => handleSelectFile(def.name)}
              style={{
                padding: '12px 14px', borderRadius: 10,
                cursor: 'pointer',
                backgroundColor: isSelected ? 'var(--accent-bg)' : 'var(--bg-elevated)',
                border: isSelected ? '1px solid var(--accent)' : '1px solid var(--border-subtle)',
                transition: 'all 0.15s',
                display: 'flex', alignItems: 'flex-start', gap: 10,
              }}
            >
              {/* 文件图标 */}
              <div style={{
                width: 32, height: 32, borderRadius: 8, flexShrink: 0,
                background: exists && !isEmpty ? 'var(--accent-bg)' : 'var(--bg-glass)',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
              }}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none"
                  stroke={exists && !isEmpty ? 'var(--accent)' : 'var(--text-muted)'}
                  strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <path d={def.icon} />
                </svg>
              </div>
              {/* 文件信息 */}
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{
                  fontSize: 13, fontWeight: 600,
                  color: exists ? 'var(--text-primary)' : 'var(--text-muted)',
                  display: 'flex', alignItems: 'center', gap: 6,
                }}>
                  {def.name}
                  {isEmpty && (
                    <span style={{ fontSize: 10, padding: '1px 5px', borderRadius: 4, backgroundColor: 'var(--warning-bg)', color: 'var(--warning)' }}>
                      empty
                    </span>
                  )}
                  {!exists && (
                    <span style={{ fontSize: 10, padding: '1px 5px', borderRadius: 4, backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)' }}>
                      new
                    </span>
                  )}
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {def.desc}
                </div>
              </div>
              {/* 大小 */}
              <span style={{ fontSize: 10, color: 'var(--text-muted)', flexShrink: 0, marginTop: 2 }}>
                {exists ? (size >= 1024 ? `${(size / 1024).toFixed(1)}KB` : `${size}B`) : ''}
              </span>
            </div>
          )
        })}
      </div>

      {/* 文件编辑器 */}
      {selectedFile && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
          <div style={{ fontSize: '12px', fontWeight: 600, marginBottom: '4px', color: 'var(--text-primary)' }}>
            {selectedFile}
          </div>
          <textarea
            value={fileContent}
            onChange={e => setFileContent(e.target.value)}
            style={{
              flex: 1, minHeight: '180px', padding: '12px', border: '1px solid var(--border-subtle)',
              borderRadius: 8, fontSize: '13px', fontFamily: "'SF Mono', Monaco, 'Cascadia Code', monospace",
              resize: 'vertical', boxSizing: 'border-box', width: '100%',
              backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
              lineHeight: 1.6,
            }}
          />
          <button
            onClick={handleSaveRaw}
            disabled={saving}
            style={{
              marginTop: '8px', width: '100%', padding: '8px', backgroundColor: 'var(--accent)',
              color: 'white', border: 'none', borderRadius: '4px', fontSize: '13px',
              cursor: saving ? 'not-allowed' : 'pointer', opacity: saving ? 0.6 : 1,
            }}
          >
            {saving ? t('common.saving') : t('soulFile.saveBtn')}
          </button>
          {status && (
            <div style={{ fontSize: '12px', color: status.startsWith(t('soulFile.saveFailed')) ? 'var(--error)' : 'var(--success)', marginTop: '6px', textAlign: 'center' }}>
              {status}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
