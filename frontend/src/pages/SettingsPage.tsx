/**
 * 设置页面 - 动态多供应商管理
 *
 * 支持任意数量的 LLM 供应商配置，包括：
 * - 预置供应商（OpenAI、Anthropic、DeepSeek、通义千问、智谱AI、Moonshot、Ollama）
 * - 自定义供应商（自定义 Base URL）
 * - 每个供应商独立的模型列表管理
 * - 环境变量自动导入提示
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n, SUPPORTED_LOCALES, LOCALE_LABELS } from '../i18n'
import { toast } from '../hooks/useToast'
import { useTheme, type Theme } from '../hooks/useTheme'
import type { Locale } from '../i18n'

interface ProviderModel {
  id: string
  name: string
}

interface Provider {
  id: string
  name: string
  apiType: string // 'openai' | 'anthropic'
  baseUrl: string
  apiKey?: string
  apiKeyMasked?: string
  models: ProviderModel[]
  enabled: boolean
}

/** 预置供应商模板 */
const PRESET_PROVIDERS: Omit<Provider, 'apiKey' | 'apiKeyMasked'>[] = [
  {
    id: 'openai',
    name: 'OpenAI',
    apiType: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    models: [
      { id: 'gpt-4o-mini', name: 'GPT-4o Mini' },
      { id: 'gpt-4o', name: 'GPT-4o' },
      { id: 'gpt-4-turbo', name: 'GPT-4 Turbo' },
    ],
    enabled: true,
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    apiType: 'anthropic',
    baseUrl: 'https://api.anthropic.com/v1',
    models: [
      { id: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4' },
      { id: 'claude-haiku-4-20250414', name: 'Claude Haiku 4' },
      { id: 'claude-opus-4-20250514', name: 'Claude Opus 4' },
    ],
    enabled: true,
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    apiType: 'openai',
    baseUrl: 'https://api.deepseek.com',
    models: [
      { id: 'deepseek-chat', name: 'DeepSeek Chat' },
      { id: 'deepseek-reasoner', name: 'DeepSeek Reasoner' },
    ],
    enabled: true,
  },
  {
    id: 'qwen',
    name: '通义千问',
    apiType: 'openai',
    baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
    models: [
      { id: 'qwen-turbo', name: 'Qwen Turbo' },
      { id: 'qwen-plus', name: 'Qwen Plus' },
      { id: 'qwen-max', name: 'Qwen Max' },
    ],
    enabled: true,
  },
  {
    id: 'zhipu',
    name: '智谱AI',
    apiType: 'openai',
    baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    models: [
      { id: 'glm-4-flash', name: 'GLM-4 Flash' },
      { id: 'glm-4', name: 'GLM-4' },
      { id: 'glm-4-plus', name: 'GLM-4 Plus' },
    ],
    enabled: true,
  },
  {
    id: 'moonshot',
    name: 'Moonshot (Kimi)',
    apiType: 'openai',
    baseUrl: 'https://api.moonshot.cn/v1',
    models: [
      { id: 'moonshot-v1-8k', name: 'Moonshot v1 8K' },
      { id: 'moonshot-v1-32k', name: 'Moonshot v1 32K' },
      { id: 'moonshot-v1-128k', name: 'Moonshot v1 128K' },
    ],
    enabled: true,
  },
  {
    id: 'gemini',
    name: 'Google Gemini',
    apiType: 'openai',
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    models: [
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash' },
      { id: 'gemini-2.5-pro-preview-06-05', name: 'Gemini 2.5 Pro' },
      { id: 'gemini-2.5-flash-preview-05-20', name: 'Gemini 2.5 Flash' },
    ],
    enabled: true,
  },
  {
    id: 'groq',
    name: 'Groq',
    apiType: 'openai',
    baseUrl: 'https://api.groq.com/openai/v1',
    models: [
      { id: 'llama-3.3-70b-versatile', name: 'Llama 3.3 70B' },
      { id: 'mixtral-8x7b-32768', name: 'Mixtral 8x7B' },
    ],
    enabled: true,
  },
  {
    id: 'ollama',
    name: 'Ollama (本地)',
    apiType: 'openai',
    baseUrl: 'http://localhost:11434/v1',
    models: [
      { id: 'llama3', name: 'Llama 3' },
      { id: 'qwen2', name: 'Qwen 2' },
      { id: 'mistral', name: 'Mistral' },
    ],
    enabled: true,
  },
]

export default function SettingsPage() {
  const { t } = useI18n()
  const [providers, setProviders] = useState<Provider[]>([])
  const [loading, setLoading] = useState(true)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editForm, setEditForm] = useState<Provider | null>(null)
  const [message, setMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)
  const [showAddMenu, setShowAddMenu] = useState(false)
  const [newModelId, setNewModelId] = useState('')
  const [newModelName, setNewModelName] = useState('')
  const [deleteConfirm, setDeleteConfirm] = useState<{ id: string; name: string } | null>(null)

  const loadProviders = async () => {
    try {
      const list = await invoke<Provider[]>('get_providers')
      setProviders(list || [])
    } catch (err) {
      console.error('加载供应商配置失败:', err)
    }
    setLoading(false)
  }

  useEffect(() => { loadProviders() }, [])

  const handleEdit = (p: Provider) => {
    setEditingId(p.id)
    setEditForm({ ...p, apiKey: '' }) // apiKey 需要重新输入
    setMessage(null)
  }

  const handleSave = async () => {
    if (!editForm) return
    try {
      await invoke('save_provider', { provider: editForm })
      setMessage({ type: 'success', text: t('settingsExtra.configSaved', { name: editForm.name }) })
      setEditingId(null)
      setEditForm(null)
      await loadProviders()
    } catch (err) {
      setMessage({ type: 'error', text: t('settingsExtra.saveFailed') + ': ' + String(err) })
    }
  }

  const handleDelete = async (id: string, name: string) => {
    try {
      await invoke('delete_provider', { providerId: id })
      setMessage({ type: 'success', text: t('settingsExtra.deleted', { name }) })
      if (editingId === id) { setEditingId(null); setEditForm(null) }
      await loadProviders()
    } catch (err) {
      setMessage({ type: 'error', text: t('settingsExtra.deleteFailed') + ': ' + String(err) })
    }
    setDeleteConfirm(null)
  }

  const handleAddPreset = async (preset: typeof PRESET_PROVIDERS[0]) => {
    // 检查是否已存在
    if (providers.some((p) => p.id === preset.id)) {
      setMessage({ type: 'error', text: t('settingsExtra.alreadyExists', { name: preset.name }) })
      setShowAddMenu(false)
      return
    }
    try {
      await invoke('save_provider', {
        provider: { ...preset, apiKey: '' },
      })
      setMessage({ type: 'success', text: t('settingsExtra.addedNeedKey', { name: preset.name }) })
      setShowAddMenu(false)
      await loadProviders()
    } catch (err) {
      setMessage({ type: 'error', text: t('settingsExtra.addFailed') + ': ' + String(err) })
    }
  }

  const handleAddCustom = async () => {
    const customId = 'custom-' + Date.now()
    const custom: Provider = {
      id: customId,
      name: t('settingsExtra.customProvider'),
      apiType: 'openai',
      baseUrl: '',
      apiKey: '',
      models: [],
      enabled: true,
    }
    try {
      await invoke('save_provider', { provider: custom })
      setShowAddMenu(false)
      await loadProviders()
      // 自动进入编辑模式
      setEditingId(customId)
      setEditForm(custom)
    } catch (err) {
      setMessage({ type: 'error', text: t('settingsExtra.addFailed') + ': ' + String(err) })
    }
  }

  const handleToggleEnabled = async (p: Provider) => {
    try {
      await invoke('save_provider', {
        provider: { ...p, enabled: !p.enabled, apiKey: '' },
      })
      await loadProviders()
    } catch (err) {
      setMessage({ type: 'error', text: t('settingsExtra.switchFailed') + ': ' + String(err) })
    }
  }

  const addModelToForm = () => {
    if (!editForm || !newModelId.trim()) return
    setEditForm({
      ...editForm,
      models: [...editForm.models, { id: newModelId.trim(), name: newModelName.trim() || newModelId.trim() }],
    })
    setNewModelId('')
    setNewModelName('')
  }

  const removeModelFromForm = (modelId: string) => {
    if (!editForm) return
    setEditForm({
      ...editForm,
      models: editForm.models.filter((m) => m.id !== modelId),
    })
  }

  if (loading) {
    return <div style={{ padding: '20px' }}>{t('common.loading')}</div>
  }

  // 未添加的预置供应商
  const availablePresets = PRESET_PROVIDERS.filter(
    (preset) => !providers.some((p) => p.id === preset.id)
  )

  return (
    <div style={{ padding: '20px', maxWidth: '700px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
        <div>
          <h1 style={{ marginTop: 0, marginBottom: '4px' }}>{t('settings.title')}</h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: '13px', margin: 0 }}>
            {t('settings.subtitle')}
          </p>
        </div>
        <div style={{ position: 'relative' }}>
          <button
            onClick={() => setShowAddMenu(!showAddMenu)}
            style={{
              padding: '8px 16px', backgroundColor: 'var(--accent)', color: '#fff',
              border: 'none', borderRadius: '6px', fontSize: '13px', cursor: 'pointer',
            }}
          >
            {t('settings.btnAddProvider')}
          </button>
          {showAddMenu && (
            <div style={{
              position: 'absolute', right: 0, top: '100%', marginTop: '4px',
              backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)', borderRadius: '8px',
              boxShadow: '0 4px 12px rgba(0,0,0,0.1)', zIndex: 10, minWidth: '220px',
              padding: '4px 0',
            }}>
              {availablePresets.map((preset) => (
                <div
                  key={preset.id}
                  onClick={() => handleAddPreset(preset)}
                  style={{
                    padding: '8px 16px', cursor: 'pointer', fontSize: '13px',
                    display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                  }}
                  onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = '#f5f5f5' }}
                  onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent' }}
                >
                  <span>{preset.name}</span>
                  <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{preset.apiType}</span>
                </div>
              ))}
              {availablePresets.length > 0 && (
                <div style={{ borderTop: '1px solid #eee', margin: '4px 0' }} />
              )}
              <div
                onClick={handleAddCustom}
                style={{ padding: '8px 16px', cursor: 'pointer', fontSize: '13px', color: 'var(--accent)' }}
                onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = '#f5f5f5' }}
                onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent' }}
              >
                {t('settingsExtra.customProviderOpenai')}
              </div>
            </div>
          )}
        </div>
      </div>

      {message && (
        <div style={{
          padding: '10px 15px', marginBottom: '16px', borderRadius: '6px', fontSize: '13px',
          backgroundColor: message.type === 'success' ? '#d4edda' : 'var(--error-bg)',
          color: message.type === 'success' ? '#155724' : '#721c24',
          border: `1px solid ${message.type === 'success' ? '#c3e6cb' : '#f5c6cb'}`,
        }}>
          {message.text}
        </div>
      )}

      {/* 供应商列表 */}
      {providers.map((p) => {
        const isEditing = editingId === p.id
        const hasKey = !!(p.apiKeyMasked && p.apiKeyMasked !== '')
        const isOllama = p.id === 'ollama' || p.baseUrl.includes('localhost')

        return (
          <div
            key={p.id}
            style={{
              marginBottom: '12px', padding: '16px',
              border: `1px solid ${p.enabled && (hasKey || isOllama) ? '#c3e6cb' : '#e0e0e0'}`,
              borderRadius: '8px',
              backgroundColor: !p.enabled ? '#fafafa' : (hasKey || isOllama) ? '#f8fff8' : '#fff',
              opacity: p.enabled ? 1 : 0.6,
            }}
          >
            {/* 供应商头部 */}
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: isEditing ? '12px' : 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <label style={{ cursor: 'pointer', display: 'flex', alignItems: 'center' }}>
                  <input
                    type="checkbox"
                    checked={p.enabled}
                    onChange={() => handleToggleEnabled(p)}
                    style={{ marginRight: '6px' }}
                  />
                </label>
                <strong style={{ fontSize: '15px' }}>{p.name}</strong>
                <span style={{
                  fontSize: '11px', padding: '2px 6px', borderRadius: '3px',
                  backgroundColor: hasKey || isOllama ? 'var(--success)' : '#ffc107',
                  color: hasKey || isOllama ? 'white' : '#333',
                }}>
                  {isOllama ? t('settings.labelLocal') : hasKey ? t('settings.labelConfigured') : t('settings.labelNoKey')}
                </span>
                <span style={{ fontSize: '11px', color: 'var(--text-muted)', padding: '2px 6px', backgroundColor: 'var(--bg-glass)', borderRadius: '3px' }}>
                  {p.apiType}
                </span>
              </div>
              <div style={{ display: 'flex', gap: '6px' }}>
                {!isEditing && (
                  <button
                    onClick={() => handleEdit(p)}
                    style={{
                      padding: '4px 12px', fontSize: '12px', cursor: 'pointer',
                      border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)',
                    }}
                  >
                    {t('common.edit')}
                  </button>
                )}
                <button
                  onClick={() => setDeleteConfirm({ id: p.id, name: p.name })}
                  style={{
                    padding: '4px 8px', fontSize: '12px', cursor: 'pointer',
                    border: '1px solid #f5c6cb', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)', color: 'var(--error)',
                  }}
                >
                  {t('common.delete')}
                </button>
              </div>
            </div>

            {/* 非编辑模式：显示模型列表摘要 */}
            {!isEditing && (
              <div style={{ marginTop: '8px', display: 'flex', flexWrap: 'wrap', gap: '4px', alignItems: 'center' }}>
                {(!p.models || p.models.length === 0) && (
                  <span style={{
                    fontSize: '12px', padding: '3px 10px', borderRadius: '4px',
                    backgroundColor: '#fff3cd', color: '#856404', border: '1px solid #ffc107',
                  }}>
                    {t('settings.warningNoModels')}
                  </span>
                )}
                {p.models?.map((m) => (
                  <span key={m.id} style={{
                    fontSize: '11px', padding: '2px 8px', borderRadius: '10px',
                    backgroundColor: '#e9ecef', color: '#495057',
                  }}>
                    {m.name || m.id}
                  </span>
                ))}
                <span style={{ fontSize: '11px', color: 'var(--text-muted)', padding: '2px 4px' }}>
                  {p.baseUrl}
                </span>
              </div>
            )}

            {/* 编辑模式 */}
            {isEditing && editForm && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '10px' }}>
                  <div>
                    <label style={{ fontSize: '12px', color: 'var(--text-secondary)', display: 'block', marginBottom: '4px' }}>{t('common.name')}</label>
                    <input
                      value={editForm.name}
                      onChange={(e) => setEditForm({ ...editForm, name: e.target.value })}
                      style={{ width: '100%', padding: '6px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' }}
                    />
                  </div>
                  <div>
                    <label style={{ fontSize: '12px', color: 'var(--text-secondary)', display: 'block', marginBottom: '4px' }}>{t('settings.fieldApiType')}</label>
                    <select
                      value={editForm.apiType}
                      onChange={(e) => setEditForm({ ...editForm, apiType: e.target.value })}
                      style={{ width: '100%', padding: '6px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' }}
                    >
                      <option value="openai">{t('settings.apiTypeOpenai')}</option>
                      <option value="anthropic">{t('settings.apiTypeAnthropic')}</option>
                    </select>
                  </div>
                </div>

                <div>
                  <label style={{ fontSize: '12px', color: 'var(--text-secondary)', display: 'block', marginBottom: '4px' }}>{t('settings.fieldBaseUrl')}</label>
                  <input
                    value={editForm.baseUrl}
                    onChange={(e) => setEditForm({ ...editForm, baseUrl: e.target.value })}
                    placeholder="https://api.example.com/v1"
                    style={{ width: '100%', padding: '6px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' }}
                  />
                </div>

                <div>
                  <label style={{ fontSize: '12px', color: 'var(--text-secondary)', display: 'block', marginBottom: '4px' }}>
                    {t('settings.fieldApiKey')} {p.apiKeyMasked && <span style={{ color: 'var(--text-muted)' }}>({t('settings.labelCurrent')}: {p.apiKeyMasked}, {t('settings.placeholderKeep')})</span>}
                  </label>
                  <input
                    type="password"
                    value={editForm.apiKey || ''}
                    onChange={(e) => setEditForm({ ...editForm, apiKey: e.target.value })}
                    placeholder={p.apiKeyMasked ? t('settings.placeholderKeep') : t('settings.placeholderEnterKey')}
                    style={{ width: '100%', padding: '6px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box' }}
                  />
                </div>

                {/* 模型列表编辑 */}
                <div>
                  <label style={{ fontSize: '12px', color: 'var(--text-secondary)', display: 'block', marginBottom: '6px' }}>
                    {t('settings.fieldModels')} <span style={{ color: 'var(--error)' }}>*</span>
                  </label>
                  {editForm.models.length === 0 && (
                    <div style={{
                      padding: '8px 12px', marginBottom: '8px', borderRadius: '4px',
                      backgroundColor: '#fff3cd', color: '#856404', fontSize: '12px',
                      border: '1px solid #ffc107',
                    }}>
                      {t('settings.warningAddModels')}
                    </div>
                  )}
                  <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px', marginBottom: '8px' }}>
                    {editForm.models.map((m) => (
                      <span key={m.id} style={{
                        fontSize: '12px', padding: '3px 8px', borderRadius: '4px',
                        backgroundColor: '#e9ecef', display: 'flex', alignItems: 'center', gap: '4px',
                      }}>
                        {m.name || m.id}
                        <span
                          onClick={() => removeModelFromForm(m.id)}
                          style={{ cursor: 'pointer', color: 'var(--error)', fontWeight: 'bold', fontSize: '14px', lineHeight: 1 }}
                        >
                          ×
                        </span>
                      </span>
                    ))}
                  </div>
                  <div style={{ display: 'flex', gap: '6px' }}>
                    <input
                      value={newModelId}
                      onChange={(e) => setNewModelId(e.target.value)}
                      placeholder={t('settings.fieldModelId')}
                      style={{ flex: 1, padding: '5px 8px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '12px' }}
                      onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addModelToForm() } }}
                    />
                    <input
                      value={newModelName}
                      onChange={(e) => setNewModelName(e.target.value)}
                      placeholder={t('settings.fieldModelDisplayName')}
                      style={{ flex: 1, padding: '5px 8px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '12px' }}
                      onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addModelToForm() } }}
                    />
                    <button
                      onClick={addModelToForm}
                      disabled={!newModelId.trim()}
                      style={{
                        padding: '5px 10px', fontSize: '12px', cursor: newModelId.trim() ? 'pointer' : 'not-allowed',
                        border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: newModelId.trim() ? '#e9ecef' : '#f5f5f5',
                      }}
                    >
                      {t('common.add')}
                    </button>
                  </div>
                </div>

                {/* 保存/取消 */}
                <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end' }}>
                  <button
                    onClick={() => { setEditingId(null); setEditForm(null) }}
                    style={{
                      padding: '6px 16px', fontSize: '13px', cursor: 'pointer',
                      border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)',
                    }}
                  >
                    {t('common.cancel')}
                  </button>
                  <button
                    onClick={handleSave}
                    style={{
                      padding: '6px 16px', fontSize: '13px', cursor: 'pointer',
                      border: 'none', borderRadius: '4px', backgroundColor: 'var(--accent)', color: '#fff',
                    }}
                  >
                    {t('common.save')}
                  </button>
                </div>
              </div>
            )}
          </div>
        )
      })}

      {providers.length === 0 && (
        <div style={{ textAlign: 'center', color: 'var(--text-muted)', padding: '40px 0' }}>
          {t('settings.emptyProviders')}
        </div>
      )}

      {/* 删除确认弹窗 */}
      {deleteConfirm && (
        <div style={{
          position: 'fixed', inset: 0, backgroundColor: 'rgba(0,0,0,0.4)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 1000,
        }}>
          <div style={{
            backgroundColor: 'white', borderRadius: 12, padding: 24,
            maxWidth: 400, width: '90%',
          }}>
            <h3 style={{ margin: '0 0 8px' }}>{t('agents.confirmDeleteTitle')}</h3>
            <p style={{ color: 'var(--text-secondary)', margin: '0 0 20px', fontSize: 14 }}>
              {t('settings.confirmDeleteProvider', { name: deleteConfirm.name })}
            </p>
            <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
              <button
                onClick={() => setDeleteConfirm(null)}
                style={{
                  padding: '8px 16px', border: '1px solid var(--border-subtle)', borderRadius: 6,
                  backgroundColor: 'white', cursor: 'pointer', fontSize: 13,
                }}
              >
                {t('common.cancel')}
              </button>
              <button
                onClick={() => handleDelete(deleteConfirm.id, deleteConfirm.name)}
                style={{
                  padding: '8px 16px', border: 'none', borderRadius: 6,
                  backgroundColor: 'var(--error)', color: 'white', cursor: 'pointer', fontSize: 13,
                }}
              >
                {t('common.delete')}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 环境变量提示 */}
      <div style={{
        marginTop: '20px', padding: '12px', backgroundColor: '#f8f9fa',
        borderRadius: '6px', fontSize: '13px', color: 'var(--text-secondary)',
      }}>
        <strong>{t('settings.hintEnvVars')}</strong>
        <pre style={{
          margin: '8px 0 0', padding: '8px', backgroundColor: '#e9ecef',
          borderRadius: '4px', fontSize: '12px', overflow: 'auto',
        }}>
{`export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export DEEPSEEK_API_KEY="sk-..."`}
        </pre>
      </div>

      {/* 语言设置 */}
      <LanguageSettings />

      {/* 主题设置 */}
      <ThemeSettings />

      {/* 心跳自治 */}
      <HeartbeatSettings />

      {/* 高级设置 */}
      <AdvancedSettings />
    </div>
  )
}

/** 心跳自治设置 */
function HeartbeatSettings() {
  const { t } = useI18n()
  const [config, setConfig] = useState({
    enabled: false,
    interval_secs: 1800,
    quiet_hours_start: 23,
    quiet_hours_end: 7,
    suppress_ok: true,
  })
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    invoke<string>('get_setting', { key: 'heartbeat_config' }).then(json => {
      if (json) {
        try { setConfig(prev => ({ ...prev, ...JSON.parse(json) })) } catch {}
      }
      setLoaded(true)
    }).catch(() => setLoaded(true))
  }, [])

  const save = async (patch: Partial<typeof config>) => {
    const updated = { ...config, ...patch }
    setConfig(updated)
    try {
      await invoke('set_setting', { key: 'heartbeat_config', value: JSON.stringify(updated) })
      toast.success(t('common.saved'))
    } catch (e) { toast.error(String(e)) }
  }

  if (!loaded) return null

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {'\u{1F49A}'} {t('settings.sectionHeartbeat')}
      </h3>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', margin: '0 0 12px' }}>
        {t('settings.heartbeatDesc')}
      </p>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {/* 启用开关 */}
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, cursor: 'pointer' }}>
          <input type="checkbox" checked={config.enabled} onChange={e => save({ enabled: e.target.checked })} />
          {t('settings.heartbeatEnabled')}
        </label>

        {config.enabled && (<>
          {/* 间隔 */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 13, color: 'var(--text-secondary)', minWidth: 80 }}>{t('settings.heartbeatInterval')}</span>
            <select
              value={config.interval_secs}
              onChange={e => save({ interval_secs: Number(e.target.value) })}
              style={{ padding: '4px 8px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
            >
              <option value={600}>10 min</option>
              <option value={1800}>30 min</option>
              <option value={3600}>1 hour</option>
              <option value={7200}>2 hours</option>
            </select>
          </div>

          {/* 静默时段 */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ fontSize: 13, color: 'var(--text-secondary)', minWidth: 80 }}>{t('settings.heartbeatQuiet')}</span>
            <input type="number" min={0} max={23} value={config.quiet_hours_start}
              onChange={e => save({ quiet_hours_start: Number(e.target.value) })}
              style={{ width: 50, padding: '4px 8px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
            />
            <span style={{ fontSize: 13 }}>—</span>
            <input type="number" min={0} max={23} value={config.quiet_hours_end}
              onChange={e => save({ quiet_hours_end: Number(e.target.value) })}
              style={{ width: 50, padding: '4px 8px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
            />
          </div>

          {/* 抑制正常结果 */}
          <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, cursor: 'pointer' }}>
            <input type="checkbox" checked={config.suppress_ok} onChange={e => save({ suppress_ok: e.target.checked })} />
            {t('settings.heartbeatSuppressOk')}
          </label>
        </>)}
      </div>
    </div>
  )
}

/** 高级设置面板（嵌入配置 + 系统状态 + 缓存统计） */
function AdvancedSettings() {
  const { t } = useI18n()
  const [expanded, setExpanded] = useState(false)
  const [embeddingKey, setEmbeddingKey] = useState('')
  const [embeddingUrl, setEmbeddingUrl] = useState('')
  const [embeddingModel, setEmbeddingModel] = useState('')
  const [embeddingDimensions, setEmbeddingDimensions] = useState('')
  const [dailyLimit, setDailyLimit] = useState('')
  const [cloudUrl, setCloudUrl] = useState('')
  const [cloudKey, setCloudKey] = useState('')
  const [health, setHealth] = useState<{ db: boolean; agents: number; memories: number; today_tokens: number } | null>(null)
  const [cacheStats, setCacheStats] = useState<{ response_cache?: { entries: number; total_hits: number }; embedding_cache?: { entries: number } } | null>(null)
  const [saving, setSaving] = useState(false)

  const loadSettings = async () => {
    try {
      const settings = await invoke<Record<string, string>>('get_settings_by_prefix', { prefix: 'embedding_' })
      setEmbeddingKey(settings?.embedding_api_key || '')
      setEmbeddingUrl(settings?.embedding_api_url || '')
      setEmbeddingModel(settings?.embedding_model || '')
      setEmbeddingDimensions(settings?.embedding_dimensions || '')
      const cloud = await invoke<Record<string, string>>('get_settings_by_prefix', { prefix: 'cloud_' })
      setCloudUrl(cloud?.cloud_gateway_url || '')
      setCloudKey(cloud?.cloud_api_key || '')
    } catch (e) { console.error(e) }
    try {
      const limit = await invoke<string | null>('get_setting', { key: 'daily_token_limit' })
      setDailyLimit(limit || '')
    } catch (e) { console.error(e) }
    try {
      setHealth(await invoke('health_check'))
      setCacheStats(await invoke('get_cache_stats'))
    } catch (e) { console.error(e) }
  }

  const saveSettings = async () => {
    setSaving(true)
    try {
      if (embeddingKey) await invoke('set_setting', { key: 'embedding_api_key', value: embeddingKey })
      if (embeddingUrl) await invoke('set_setting', { key: 'embedding_api_url', value: embeddingUrl })
      if (embeddingModel) await invoke('set_setting', { key: 'embedding_model', value: embeddingModel })
      if (embeddingDimensions) await invoke('set_setting', { key: 'embedding_dimensions', value: embeddingDimensions })
      if (dailyLimit) await invoke('set_setting', { key: 'daily_token_limit', value: dailyLimit })
      if (cloudUrl) await invoke('set_setting', { key: 'cloud_gateway_url', value: cloudUrl })
      if (cloudKey) await invoke('set_setting', { key: 'cloud_api_key', value: cloudKey })
      toast.success(t('settings.successSaved'))
    } catch (e: unknown) {
      toast.error(t('settingsExtra.saveFailed') + ': ' + ((e as Error)?.message || e))
    }
    setSaving(false)
  }

  useEffect(() => { if (expanded) loadSettings() }, [expanded])

  if (!expanded) {
    return (
      <div style={{ marginTop: '20px', textAlign: 'center' }}>
        <button onClick={() => setExpanded(true)} style={{
          padding: '8px 20px', background: 'none', border: '1px solid #ccc',
          borderRadius: '4px', cursor: 'pointer', color: 'var(--text-secondary)',
        }}>
          {t('settings.sectionAdvanced')}
        </button>
      </div>
    )
  }

  return (
    <div style={{ marginTop: '20px', padding: '16px', border: '1px solid #e0e0e0', borderRadius: '8px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
        <h3 style={{ margin: 0 }}>{t('settings.sectionAdvanced')}</h3>
        <button onClick={() => setExpanded(false)} style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: '18px' }}>×</button>
      </div>

      {/* 向量嵌入 */}
      <div style={{ marginBottom: '16px' }}>
        <h4 style={{ margin: '0 0 8px', color: 'var(--text-secondary)' }}>{t('settings.sectionEmbedding')}</h4>
        <p style={{ fontSize: '12px', color: 'var(--text-muted)', margin: '0 0 8px' }}>
          {t('settings.hintEmbedding')}
        </p>
        <input placeholder={t('settingsExtra.embeddingKeyPlaceholder')} value={embeddingKey} onChange={e => setEmbeddingKey(e.target.value)}
          style={{ width: '100%', padding: '6px', marginBottom: '6px', boxSizing: 'border-box' }} type="password" />
        <div style={{ display: 'flex', gap: '8px', marginBottom: '8px' }}>
          <input placeholder={t('settingsExtra.embeddingUrlPlaceholder')} value={embeddingUrl} onChange={e => setEmbeddingUrl(e.target.value)}
            style={{ flex: 3, padding: '6px' }} />
          <input placeholder={t('settingsExtra.embeddingModelPlaceholder')} value={embeddingModel} onChange={e => setEmbeddingModel(e.target.value)}
            style={{ flex: 2, padding: '6px' }} />
          <input placeholder={t('settingsExtra.embeddingDimPlaceholder')} value={embeddingDimensions} onChange={e => setEmbeddingDimensions(e.target.value)}
            style={{ flex: 1, padding: '6px' }} type="number" />
        </div>
        <button
          onClick={async () => {
            if (!embeddingKey || !embeddingUrl) { toast.info(t('settingsExtra.fillKeyFirst')); return }
            try {
              const res = await fetch(embeddingUrl, {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${embeddingKey}`, 'Content-Type': 'application/json' },
                body: JSON.stringify({ model: embeddingModel || 'text-embedding-3-small', input: 'test', dimensions: parseInt(embeddingDimensions) || 1024 }),
              })
              const data = await res.json()
              if (data?.data?.[0]?.embedding) {
                toast.success(t('settingsExtra.connectionSuccess', { dim: String(data.data[0].embedding.length), token: String(data.usage?.total_tokens || '?') }))
              } else {
                toast.error(t('settingsExtra.connectionFailed') + ': ' + JSON.stringify(data).substring(0, 200))
              }
            } catch (e: unknown) { toast.error(t('settingsExtra.connectionFailed') + ': ' + ((e as Error)?.message || e)) }
          }}
          style={{ padding: '4px 12px', fontSize: '12px', border: '1px solid var(--border-subtle)', borderRadius: '4px', cursor: 'pointer', backgroundColor: 'var(--bg-elevated)' }}
        >
          {t('settings.btnTestConnection')}
        </button>
      </div>

      {/* 云端连接（混合架构） */}
      <div style={{ marginBottom: '16px' }}>
        <h4 style={{ margin: '0 0 8px', color: 'var(--text-secondary)' }}>{t('settings.sectionCloud')}</h4>
        <p style={{ fontSize: '12px', color: 'var(--text-muted)', margin: '0 0 8px' }}>
          {t('settings.hintCloud')}
        </p>
        <input placeholder={t('settingsExtra.gatewayPlaceholder')} value={cloudUrl} onChange={e => setCloudUrl(e.target.value)}
          style={{ width: '100%', padding: '6px', marginBottom: '6px', boxSizing: 'border-box' }} />
        <input placeholder="API Key" value={cloudKey} onChange={e => setCloudKey(e.target.value)}
          style={{ width: '100%', padding: '6px', boxSizing: 'border-box' }} type="password" />
      </div>

      {/* Token 限额 */}
      <div style={{ marginBottom: '16px' }}>
        <h4 style={{ margin: '0 0 8px', color: 'var(--text-secondary)' }}>{t('settings.sectionDailyLimit')}</h4>
        <input placeholder={t('settings.fieldDailyLimit')} value={dailyLimit} onChange={e => setDailyLimit(e.target.value)}
          style={{ width: '200px', padding: '6px' }} type="number" />
      </div>

      <button onClick={saveSettings} disabled={saving} style={{
        padding: '8px 20px', backgroundColor: 'var(--success)', color: 'white',
        border: 'none', borderRadius: '4px', cursor: saving ? 'not-allowed' : 'pointer',
        marginBottom: '16px',
      }}>
        {saving ? t('common.saving') : t('common.save')}
      </button>

      {/* 系统状态 */}
      {health && (
        <div style={{ marginTop: '12px', padding: '12px', backgroundColor: '#f8f9fa', borderRadius: '6px', fontSize: '13px' }}>
          <h4 style={{ margin: '0 0 8px' }}>{t('settings.sectionSystemStatus')}</h4>
          <div>{t('settings.labelDatabase')}: {health.db ? t('common.healthy') : t('common.error')} | {t('settings.labelAgents')}: {health.agents} | {t('settings.labelMemories')}: {health.memories} | {t('settings.labelTodayTokens')}: {health.today_tokens?.toLocaleString()}</div>
          {cacheStats && (
            <div style={{ marginTop: '4px' }}>
              {t('settings.labelResponseCache')}: {cacheStats.response_cache?.entries}{t('common.entries')} ({cacheStats.response_cache?.total_hits}{t('settings.labelHits')}) |
              {t('settings.labelEmbeddingCache')}: {cacheStats.embedding_cache?.entries}{t('common.entries')}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

/** 语言设置 */
function LanguageSettings() {
  const { locale, setLocale, t } = useI18n()

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {'\u{1F310}'}  {t('settings.sectionLanguage')}
      </h3>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{t('settings.labelLanguage')}</span>
        <select
          value={locale}
          onChange={e => setLocale(e.target.value as Locale)}
          style={{ padding: '6px 12px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13, cursor: 'pointer' }}
        >
          {SUPPORTED_LOCALES.map(loc => (
            <option key={loc} value={loc}>{LOCALE_LABELS[loc]}</option>
          ))}
        </select>
      </div>
    </div>
  )
}

function ThemeSettings() {
  const { t } = useI18n()
  const { theme, setTheme } = useTheme()

  const themes: { value: Theme; label: string; icon: string }[] = [
    { value: 'light', label: t('settings.themeLight'), icon: '\u2600\uFE0F' },
    { value: 'dark', label: t('settings.themeDark'), icon: '\u{1F319}' },
    { value: 'system', label: t('settings.themeSystem'), icon: '\u{1F4BB}' },
  ]

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {'\u{1F3A8}'}  {t('settings.sectionTheme')}
      </h3>
      <div style={{ display: 'flex', gap: 8 }}>
        {themes.map(opt => (
          <button
            key={opt.value}
            onClick={() => setTheme(opt.value)}
            style={{
              flex: 1,
              padding: '10px 12px',
              borderRadius: 8,
              border: theme === opt.value ? '2px solid var(--accent)' : '1px solid var(--border-subtle)',
              backgroundColor: theme === opt.value ? 'var(--accent-bg)' : 'var(--bg-elevated)',
              color: 'var(--text-primary)',
              cursor: 'pointer',
              fontSize: 13,
              fontWeight: theme === opt.value ? 600 : 400,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 6,
            }}
          >
            <span>{opt.icon}</span>
            <span>{opt.label}</span>
          </button>
        ))}
      </div>
    </div>
  )
}
