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

/** 所有灵魂文件 */
const ALL_SOUL_FILES = [
  'IDENTITY.md', 'SOUL.md', 'AGENTS.md', 'USER.md',
  'TOOLS.md', 'MEMORY.md', 'BOOTSTRAP.md', 'HEARTBEAT.md',
]

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
    width: '100%', padding: '6px 8px', border: '1px solid #ddd',
    borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' as const,
  }
  const labelStyle = { fontSize: '12px', color: '#666', marginBottom: '3px', display: 'block' }
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
              fontSize: '11px', padding: '3px 8px', border: '1px solid #ddd',
              borderRadius: '3px', background: '#f8f8f8', cursor: 'pointer', color: '#666',
            }}
          >
            {t('soulFile.rawMode')}
          </button>
        </div>

        {/* IDENTITY 区域 */}
        <div style={{ marginBottom: '14px' }}>
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: '#333' }}>{t('soulFile.sectionIdentity')}</div>
          {form.customIdentity ? (
            <div>
              <div style={{ fontSize: '11px', color: '#999', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
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
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: '#333' }}>{t('soulFile.sectionSoul')}</div>
          {form.customSoul ? (
            <div>
              <div style={{ fontSize: '11px', color: '#999', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
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
          <div style={{ fontSize: '13px', fontWeight: 600, marginBottom: '8px', color: '#333' }}>{t('soulFile.sectionUser')}</div>
          {form.customUser ? (
            <div>
              <div style={{ fontSize: '11px', color: '#999', marginBottom: '4px' }}>{t('soulFile.customFormatHint')}</div>
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
            fontSize: '11px', padding: '3px 8px', border: '1px solid #ddd',
            borderRadius: '3px', background: '#f8f8f8', cursor: 'pointer', color: '#666',
          }}
        >
          {t('soulFile.formMode')}
        </button>
      </div>

      {/* 文件列表 */}
      <div style={{ marginBottom: '10px' }}>
        {ALL_SOUL_FILES.map(f => {
          const exists = fileList.includes(f)
          const isSelected = selectedFile === f
          return (
            <div
              key={f}
              onClick={() => handleSelectFile(f)}
              style={{
                padding: '6px 8px', marginBottom: '2px', borderRadius: '3px',
                cursor: 'pointer', fontSize: '12px',
                backgroundColor: isSelected ? '#e3f2fd' : 'transparent',
                color: exists ? '#333' : '#bbb',
                display: 'flex', justifyContent: 'space-between', alignItems: 'center',
              }}
            >
              <span>{f}</span>
              <span style={{ fontSize: '10px', color: '#999' }}>
                {exists ? `${fileSizes[f] || 0}B` : t('soulFile.notCreated')}
              </span>
            </div>
          )
        })}
      </div>

      {/* 文件编辑器 */}
      {selectedFile && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
          <div style={{ fontSize: '12px', fontWeight: 600, marginBottom: '4px', color: '#333' }}>
            {selectedFile}
          </div>
          <textarea
            value={fileContent}
            onChange={e => setFileContent(e.target.value)}
            style={{
              flex: 1, minHeight: '150px', padding: '8px', border: '1px solid #ddd',
              borderRadius: '4px', fontSize: '12px', fontFamily: 'monospace',
              resize: 'vertical', boxSizing: 'border-box', width: '100%',
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
