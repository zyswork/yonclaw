/**
 * 设置页面 - 动态多供应商管理
 *
 * 支持任意数量的 LLM 供应商配置，包括：
 * - 预置供应商（OpenAI、Anthropic、DeepSeek、通义千问、智谱AI、Moonshot、Ollama）
 * - 自定义供应商（自定义 Base URL）
 * - 每个供应商独立的模型列表管理
 * - 环境变量自动导入提示
 */

import { useEffect, useState, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { useLocation } from 'react-router-dom'
import { useI18n, SUPPORTED_LOCALES, LOCALE_LABELS } from '../i18n'
import { toast, friendlyError } from '../hooks/useToast'
import { useTheme, type Theme } from '../hooks/useTheme'
import type { Locale } from '../i18n'
import Select from '../components/Select'
import ProviderModelSelector from '../components/ProviderModelSelector'
import { useAuthStore } from '../store/authStore'

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

/** OAuth 供应商模板（特殊处理，不走 API Key 流程） */
const OAUTH_PROVIDERS = [
  { id: 'google-oauth', name: 'Google Gemini (OAuth)', apiType: 'openai', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', isOAuth: true },
  { id: 'openai-oauth', name: 'OpenAI (OAuth)', apiType: 'openai', baseUrl: '', isOAuth: true },
] as const

/** 预置供应商模板 */
const PRESET_PROVIDERS: Omit<Provider, 'apiKey' | 'apiKeyMasked'>[] = [
  {
    id: 'openai',
    name: 'OpenAI',
    apiType: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    models: [
      { id: 'gpt-5.4', name: 'GPT-5.4' },
      { id: 'gpt-5.2', name: 'GPT-5.2' },
      { id: 'gpt-5.1-codex', name: 'GPT-5.1 Codex' },
      { id: 'gpt-5.1-codex-mini', name: 'GPT-5.1 Codex Mini' },
      { id: 'gpt-4o', name: 'GPT-4o' },
      { id: 'gpt-4o-mini', name: 'GPT-4o Mini' },
    ],
    enabled: true,
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    apiType: 'anthropic',
    baseUrl: 'https://api.anthropic.com/v1',
    models: [
      { id: 'claude-opus-4-6', name: 'Claude Opus 4.6' },
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6' },
      { id: 'claude-haiku-4-5-20251001', name: 'Claude Haiku 4.5' },
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
      { id: 'qwen3.5-plus', name: 'Qwen 3.5 Plus' },
      { id: 'qwen3-coder-plus', name: 'Qwen 3 Coder Plus' },
      { id: 'qwen-max', name: 'Qwen Max' },
      { id: 'qwen-plus', name: 'Qwen Plus' },
    ],
    enabled: true,
  },
  {
    id: 'zhipu',
    name: '智谱 AI (GLM)',
    apiType: 'openai',
    baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    models: [
      { id: 'glm-5', name: 'GLM-5' },
      { id: 'glm-5-turbo', name: 'GLM-5 Turbo' },
      { id: 'glm-4.7', name: 'GLM-4.7' },
      { id: 'glm-4.7-flash', name: 'GLM-4.7 Flash' },
      { id: 'glm-4.7-flashx', name: 'GLM-4.7 FlashX' },
      { id: 'glm-4.6', name: 'GLM-4.6' },
      { id: 'glm-z1', name: 'GLM-Z1 (推理)' },
    ],
    enabled: true,
  },
  {
    id: 'zhipu-coding',
    name: '智谱 AI (CodePlan)',
    apiType: 'openai',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    models: [
      { id: 'glm-5', name: 'GLM-5' },
      { id: 'glm-5-turbo', name: 'GLM-5 Turbo' },
      { id: 'glm-4.7', name: 'GLM-4.7' },
      { id: 'glm-4.7-flash', name: 'GLM-4.7 Flash' },
      { id: 'glm-4.6', name: 'GLM-4.6' },
    ],
    enabled: true,
  },
  {
    id: 'moonshot',
    name: 'Moonshot (Kimi)',
    apiType: 'openai',
    baseUrl: 'https://api.moonshot.cn/v1',
    models: [
      { id: 'kimi-k2.5', name: 'Kimi K2.5' },
      { id: 'kimi-k2.5-thinking', name: 'Kimi K2.5 Thinking' },
      { id: 'moonshot-v1-128k', name: 'Moonshot v1 128K' },
      { id: 'moonshot-v1-32k', name: 'Moonshot v1 32K' },
    ],
    enabled: true,
  },
  {
    id: 'minimax',
    name: 'MiniMax',
    apiType: 'openai',
    baseUrl: 'https://api.minimax.chat/v1',
    models: [
      { id: 'MiniMax-M2.7', name: 'MiniMax M2.7' },
      { id: 'MiniMax-M2.5', name: 'MiniMax M2.5' },
      { id: 'MiniMax-M2.7-highspeed', name: 'M2.7 HighSpeed' },
      { id: 'MiniMax-M2.5-highspeed', name: 'M2.5 HighSpeed' },
    ],
    enabled: true,
  },
  {
    id: 'baichuan',
    name: '百川智能',
    apiType: 'openai',
    baseUrl: 'https://api.baichuan-ai.com/v1',
    models: [
      { id: 'Baichuan4-Turbo', name: 'Baichuan 4 Turbo' },
      { id: 'Baichuan4-Air', name: 'Baichuan 4 Air' },
    ],
    enabled: true,
  },
  {
    id: 'stepfun',
    name: '阶跃星辰 (Step)',
    apiType: 'openai',
    baseUrl: 'https://api.stepfun.com/v1',
    models: [
      { id: 'step-2-16k', name: 'Step 2 16K' },
      { id: 'step-1-128k', name: 'Step 1 128K' },
    ],
    enabled: true,
  },
  {
    id: 'doubao',
    name: '豆包 (ByteDance)',
    apiType: 'openai',
    baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
    models: [
      { id: 'doubao-1.5-pro-256k', name: 'Doubao 1.5 Pro 256K' },
      { id: 'doubao-1.5-lite-32k', name: 'Doubao 1.5 Lite 32K' },
    ],
    enabled: true,
  },
  {
    id: 'gemini',
    name: 'Google Gemini',
    apiType: 'openai',
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    models: [
      { id: 'gemini-3.1-pro-preview', name: 'Gemini 3.1 Pro' },
      { id: 'gemini-2.5-pro-preview-06-05', name: 'Gemini 2.5 Pro' },
      { id: 'gemini-2.5-flash-preview-05-20', name: 'Gemini 2.5 Flash' },
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash' },
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
    id: 'mistral',
    name: 'Mistral AI',
    apiType: 'openai',
    baseUrl: 'https://api.mistral.ai/v1',
    models: [
      { id: 'mistral-large-latest', name: 'Mistral Large' },
      { id: 'mistral-small-latest', name: 'Mistral Small' },
      { id: 'codestral-latest', name: 'Codestral' },
    ],
    enabled: true,
  },
  {
    id: 'xai',
    name: 'xAI (Grok)',
    apiType: 'openai',
    baseUrl: 'https://api.x.ai/v1',
    models: [
      { id: 'grok-4', name: 'Grok 4' },
      { id: 'grok-3', name: 'Grok 3' },
      { id: 'grok-3-mini', name: 'Grok 3 Mini' },
      { id: 'grok-4-fast', name: 'Grok 4 Fast' },
    ],
    enabled: true,
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    apiType: 'openai',
    baseUrl: 'https://openrouter.ai/api/v1',
    models: [
      { id: 'anthropic/claude-opus-4-6', name: 'Claude Opus 4.6 (via OR)' },
      { id: 'openai/gpt-5.4', name: 'GPT-5.4 (via OR)' },
      { id: 'google/gemini-2.5-pro', name: 'Gemini 2.5 Pro (via OR)' },
    ],
    enabled: true,
  },
  {
    id: 'together',
    name: 'Together AI',
    apiType: 'openai',
    baseUrl: 'https://api.together.xyz/v1',
    models: [
      { id: 'meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8', name: 'Llama 4 Maverick' },
      { id: 'deepseek-ai/DeepSeek-R1', name: 'DeepSeek R1' },
      { id: 'Qwen/Qwen3-235B-A22B-FP8', name: 'Qwen3 235B' },
    ],
    enabled: true,
  },
  {
    id: 'nvidia',
    name: 'Nvidia NIM',
    apiType: 'openai',
    baseUrl: 'https://integrate.api.nvidia.com/v1',
    models: [
      { id: 'meta/llama-3.3-70b-instruct', name: 'Llama 3.3 70B' },
      { id: 'nvidia/llama-3.1-nemotron-70b-instruct', name: 'Nemotron 70B' },
    ],
    enabled: true,
  },
  {
    id: 'ollama',
    name: 'Ollama (Local)',
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

/** 分类导航项定义 */
type SectionId = 'profile' | 'providers' | 'tts' | 'appearance' | 'search' | 'heartbeat' | 'backup' | 'gateway' | 'embedding' | 'background'

interface NavItem {
  id: SectionId
  labelKey: string
  icon: JSX.Element
}

/** 左侧导航图标（SVG 线条风格） */
const NAV_ITEMS: NavItem[] = [
  {
    id: 'profile',
    labelKey: 'settings.sectionProfile',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2" />
        <circle cx="12" cy="7" r="4" />
      </svg>
    ),
  },
  {
    id: 'providers',
    labelKey: 'settings.title',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <rect x="2" y="2" width="20" height="8" rx="2" />
        <rect x="2" y="14" width="20" height="8" rx="2" />
        <circle cx="6" cy="6" r="1" fill="currentColor" />
        <circle cx="6" cy="18" r="1" fill="currentColor" />
      </svg>
    ),
  },
  {
    id: 'appearance',
    labelKey: 'settings.sectionTheme',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="5" />
        <line x1="12" y1="1" x2="12" y2="3" />
        <line x1="12" y1="21" x2="12" y2="23" />
        <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
        <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
        <line x1="1" y1="12" x2="3" y2="12" />
        <line x1="21" y1="12" x2="23" y2="12" />
        <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
        <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
      </svg>
    ),
  },
  {
    id: 'search',
    labelKey: 'settings.sectionSearch',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="11" cy="11" r="8" />
        <line x1="21" y1="21" x2="16.65" y2="16.65" />
      </svg>
    ),
  },
  {
    id: 'heartbeat',
    labelKey: 'settings.sectionHeartbeat',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" />
      </svg>
    ),
  },
  {
    id: 'backup',
    labelKey: 'settings.sectionBackup',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z" />
        <polyline points="17 21 17 13 7 13 7 21" />
        <polyline points="7 3 7 8 15 8" />
      </svg>
    ),
  },
  {
    id: 'gateway',
    labelKey: 'settings.sectionCloud',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M18 10h-1.26A8 8 0 109 20h9a5 5 0 000-10z" />
      </svg>
    ),
  },
  {
    id: 'embedding',
    labelKey: 'settings.sectionEmbedding',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="10" />
        <circle cx="12" cy="12" r="6" />
        <circle cx="12" cy="12" r="2" />
      </svg>
    ),
  },
  {
    id: 'tts' as SectionId,
    labelKey: 'settings.sectionTts',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
        <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
        <line x1="12" y1="19" x2="12" y2="23" />
        <line x1="8" y1="23" x2="16" y2="23" />
      </svg>
    ),
  },
  {
    id: 'background',
    labelKey: 'settings.sectionBackground',
    icon: (
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83" />
      </svg>
    ),
  },
]

export default function SettingsPage() {
  const { t } = useI18n()
  const location = useLocation()
  const [activeSection, setActiveSection] = useState<SectionId>('providers')
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

  // 监听 OAuth 完成事件
  useEffect(() => {
    const unlistenP = listen<any>('oauth-complete', (e) => {
      if (e.payload?.success) {
        toast.success(`${e.payload.provider || 'Provider'} ${t('settings.oauthSuccess')}`)
        loadProviders() // 刷新供应商列表
      } else {
        toast.error(`${t('settings.oauthFailed')}: ${e.payload?.error || '未知错误'}`)
      }
    })
    return () => { unlistenP.then(f => f()) }
  }, [])

  /** 启动 OAuth 授权流程 */
  const handleOAuthAdd = async (oauthProviderId: string) => {
    try {
      await invoke<{ state: string; authorizeUrl: string }>('start_oauth_flow', { provider: oauthProviderId.replace('-oauth', '') })
      toast.success(t('settings.oauthBrowserOpened'))
      // 回调由 gateway 处理，触发 'oauth-complete' 事件
    } catch (e) {
      toast.error(`${t('settings.oauthFailed')}: ${String(e)}`)
    }
    setShowAddMenu(false)
  }

  // 从 URL 参数读取 section（支持 ?section=profile 等）
  useEffect(() => {
    const params = new URLSearchParams(location.search)
    const section = params.get('section')
    const validSections: SectionId[] = ['profile', 'providers', 'tts', 'appearance', 'search', 'heartbeat', 'backup', 'gateway', 'embedding', 'background']
    if (section && validSections.includes(section as SectionId)) {
      setActiveSection(section as SectionId)
    }
  }, [location.search])

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

  /** 导航项的显示名称（需要 t 函数，所以在组件内定义映射） */
  const navLabels: Record<SectionId, string> = {
    profile: t('settings.sectionProfile'),
    providers: t('settings.title'),
    appearance: t('settings.sectionTheme') + ' / ' + t('settings.sectionLanguage'),
    search: t('settings.sectionSearch'),
    heartbeat: t('settings.sectionHeartbeat'),
    backup: t('settings.sectionBackup') || 'Backup',
    gateway: t('settings.sectionCloud'),
    embedding: t('settings.sectionEmbedding'),
    tts: '语音合成',
    background: t('settings.sectionBackground'),
  }

  return (
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden' }}>
      {/* ===== 左侧分类导航 ===== */}
      <nav style={{
        width: 200, minWidth: 200, height: '100%', overflowY: 'auto',
        backgroundColor: 'var(--bg-glass)', borderRight: '1px solid var(--border-subtle)',
        padding: '16px 0', display: 'flex', flexDirection: 'column', gap: 2,
      }}>
        <h2 style={{ margin: '0 0 12px', padding: '0 16px', fontSize: 15, fontWeight: 700, color: 'var(--text-primary)' }}>
          {t('settings.title')}
        </h2>
        {NAV_ITEMS.map(item => {
          const isActive = activeSection === item.id
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '10px 16px', margin: '0 8px',
                border: 'none', borderRadius: 8, cursor: 'pointer',
                fontSize: 13, fontWeight: isActive ? 600 : 400,
                color: isActive ? 'var(--accent)' : 'var(--text-secondary)',
                backgroundColor: isActive ? 'var(--accent-bg)' : 'transparent',
                borderLeft: isActive ? '3px solid var(--accent)' : '3px solid transparent',
                textAlign: 'left', width: 'calc(100% - 16px)',
                transition: 'all 0.15s ease',
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.backgroundColor = 'var(--bg-elevated)'
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.backgroundColor = 'transparent'
              }}
            >
              {item.icon}
              <span>{navLabels[item.id]}</span>
            </button>
          )
        })}
      </nav>

      {/* ===== 右侧内容面板 ===== */}
      <div style={{ flex: 1, overflowY: 'auto', padding: 24 }}>
        {/* 全局消息提示 */}
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

        {/* ---- 个人资料 ---- */}
        {activeSection === 'profile' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionProfile')}</h1>
            <ProfileSection />

            {/* 数据迁移 */}
            <div style={{ marginTop: 32, padding: 16, borderRadius: 8, border: '1px solid var(--border-subtle)', background: 'var(--bg-glass)' }}>
              <h3 style={{ margin: '0 0 8px', fontSize: 15 }}>数据迁移</h3>
              <p style={{ color: 'var(--text-secondary)', fontSize: 13, margin: '0 0 12px' }}>
                导出所有数据（对话、Agent 配置、记忆、个人资料）到文件，在另一台电脑导入即可使用。
              </p>
              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  onClick={async () => {
                    try {
                      const { save } = await import('@tauri-apps/api/dialog')
                      const path = await save({ defaultPath: 'xianzhu-data.zip', filters: [{ name: 'ZIP', extensions: ['zip'] }] })
                      if (!path) return
                      const result = await invoke<string>('export_app_data', { outputPath: path })
                      toast.success(result)
                    } catch (e) { toast.error(friendlyError(e)) }
                  }}
                  style={{ padding: '8px 16px', borderRadius: 6, border: '1px solid var(--accent)', background: 'transparent', color: 'var(--accent)', cursor: 'pointer', fontSize: 13 }}
                >
                  导出数据
                </button>
                <button
                  onClick={async () => {
                    try {
                      const { open } = await import('@tauri-apps/api/dialog')
                      const path = await open({ filters: [{ name: 'ZIP', extensions: ['zip'] }] })
                      if (!path || Array.isArray(path)) return
                      const result = await invoke<string>('import_app_data', { zipPath: path })
                      toast.success(result)
                    } catch (e) { toast.error(friendlyError(e)) }
                  }}
                  style={{ padding: '8px 16px', borderRadius: 6, border: '1px solid var(--border-subtle)', background: 'transparent', color: 'var(--text-primary)', cursor: 'pointer', fontSize: 13 }}
                >
                  导入数据
                </button>
              </div>
            </div>
          </div>
        )}

        {/* ---- 供应商 ---- */}
        {activeSection === 'providers' && (
          <div style={{ maxWidth: 700 }}>
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
                    {/* OAuth 供应商（置顶） */}
                    {OAUTH_PROVIDERS.map((op) => (
                      <div
                        key={op.id}
                        onClick={() => handleOAuthAdd(op.id)}
                        style={{
                          padding: '8px 16px', cursor: 'pointer', fontSize: '13px',
                          display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                        }}
                        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent' }}
                      >
                        <span>{op.name}</span>
                        <span style={{ fontSize: '11px', color: 'var(--accent)', fontWeight: 600 }}>OAuth</span>
                      </div>
                    ))}
                    <div style={{ borderTop: '1px solid var(--border-subtle)', margin: '4px 0' }} />
                    {/* 普通预置供应商 */}
                    {availablePresets.map((preset) => (
                      <div
                        key={preset.id}
                        onClick={() => handleAddPreset(preset)}
                        style={{
                          padding: '8px 16px', cursor: 'pointer', fontSize: '13px',
                          display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                        }}
                        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent' }}
                      >
                        <span>{preset.name}</span>
                        <span style={{ fontSize: '11px', color: 'var(--text-muted)' }}>{preset.apiType}</span>
                      </div>
                    ))}
                    {availablePresets.length > 0 && (
                      <div style={{ borderTop: '1px solid var(--border-subtle)', margin: '4px 0' }} />
                    )}
                    <div
                      onClick={handleAddCustom}
                      style={{ padding: '8px 16px', cursor: 'pointer', fontSize: '13px', color: 'var(--accent)' }}
                      onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                      onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent' }}
                    >
                      {t('settingsExtra.customProviderOpenai')}
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* 供应商列表 */}
            {providers.map((p) => {
              const isEditing = editingId === p.id
              const hasKey = !!(p.apiKeyMasked && p.apiKeyMasked !== '')
              const isOllama = p.id === 'ollama' || p.baseUrl.includes('localhost')
              const isOAuthProvider = (p as any).authMethod === 'oauth'

              return (
                <div
                  key={p.id}
                  style={{
                    marginBottom: '12px', padding: '16px',
                    border: `1px solid ${p.enabled && (hasKey || isOllama || isOAuthProvider) ? '#c3e6cb' : 'var(--border-subtle)'}`,
                    borderRadius: '8px',
                    backgroundColor: !p.enabled ? 'var(--bg-glass)' : (hasKey || isOllama || isOAuthProvider) ? 'var(--success-bg)' : 'var(--bg-elevated)',
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
                        backgroundColor: isOAuthProvider ? '#28a745' : (hasKey || isOllama) ? 'var(--success)' : '#ffc107',
                        color: isOAuthProvider ? '#fff' : (hasKey || isOllama) ? '#fff' : 'var(--text-primary)',
                      }}>
                        {isOAuthProvider ? t('settings.oauthAuthorized') : isOllama ? t('settings.labelLocal') : hasKey ? t('settings.labelConfigured') : t('settings.labelNoKey')}
                      </span>
                      {isOAuthProvider && (p as any).oauth?.expiresAt && (
                        <span style={{ fontSize: '10px', color: 'var(--text-muted)', padding: '2px 6px' }}>
                          {t('settings.oauthExpires')}: {new Date((p as any).oauth.expiresAt * 1000).toLocaleString()}
                        </span>
                      )}
                      <span style={{ fontSize: '11px', color: 'var(--text-muted)', padding: '2px 6px', backgroundColor: 'var(--bg-glass)', borderRadius: '3px' }}>
                        {p.apiType}
                      </span>
                    </div>
                    <div style={{ display: 'flex', gap: '6px' }}>
                      {!isEditing && (
                        <>
                          {isOAuthProvider && (
                            <button
                              onClick={() => handleOAuthAdd(p.id.endsWith('-oauth') ? p.id : p.id + '-oauth')}
                              style={{
                                padding: '4px 10px', fontSize: '11px', cursor: 'pointer',
                                border: '1px solid var(--accent)', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)', color: 'var(--accent)',
                              }}
                            >
                              {t('settings.oauthReauthorize')}
                            </button>
                          )}
                          <button
                            onClick={async () => {
                              try {
                                const result = await invoke<any>('test_provider_connection', {
                                  apiType: p.apiType, apiKey: p.apiKey || '', baseUrl: p.baseUrl || null
                                })
                                toast.success(`${p.name}: ${result.latency_ms}ms, ${result.models_available} models`)
                              } catch (e) { toast.error(`${p.name}: ${String(e)}`) }
                            }}
                            style={{
                              padding: '4px 10px', fontSize: '11px', cursor: 'pointer',
                              border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)',
                            }}
                          >
                            {t('settings.testBtn')}
                          </button>
                          <button
                            onClick={() => handleEdit(p)}
                            style={{
                              padding: '4px 12px', fontSize: '12px', cursor: 'pointer',
                              border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: 'var(--bg-elevated)',
                            }}
                          >
                            {t('common.edit')}
                          </button>
                        </>
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
                          backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)',
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
                          <Select
                            value={editForm.apiType}
                            onChange={(v) => setEditForm({ ...editForm, apiType: v })}
                            options={[
                              { value: 'openai', label: t('settings.apiTypeOpenai') },
                              { value: 'anthropic', label: t('settings.apiTypeAnthropic') },
                            ]}
                            style={{ width: '100%' }}
                          />
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
                        <textarea
                          value={(editForm.apiKey || '').split('|||').join('\n')}
                          onChange={(e) => setEditForm({ ...editForm, apiKey: e.target.value.split('\n').filter((k: string) => k.trim()).join('|||') })}
                          placeholder={p.apiKeyMasked ? t('settings.placeholderKeep') : t('settings.placeholderEnterKey')}
                          rows={3}
                          style={{ width: '100%', padding: '6px 10px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box', fontFamily: 'monospace', resize: 'vertical' }}
                        />
                        <div style={{ fontSize: '11px', color: 'var(--text-muted)', marginTop: '2px' }}>
                          {t('settings.multiKeyHint')}
                        </div>
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
                              backgroundColor: 'var(--bg-glass)', display: 'flex', alignItems: 'center', gap: '4px',
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
                            onKeyDown={(e) => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); addModelToForm() } }}
                          />
                          <input
                            value={newModelName}
                            onChange={(e) => setNewModelName(e.target.value)}
                            placeholder={t('settings.fieldModelDisplayName')}
                            style={{ flex: 1, padding: '5px 8px', border: '1px solid var(--border-subtle)', borderRadius: '4px', fontSize: '12px' }}
                            onKeyDown={(e) => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); addModelToForm() } }}
                          />
                          <button
                            onClick={addModelToForm}
                            disabled={!newModelId.trim()}
                            style={{
                              padding: '5px 10px', fontSize: '12px', cursor: newModelId.trim() ? 'pointer' : 'not-allowed',
                              border: '1px solid var(--border-subtle)', borderRadius: '4px', backgroundColor: newModelId.trim() ? 'var(--bg-glass)' : 'var(--bg-glass)',
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

            {/* 环境变量提示 */}
            <div style={{
              marginTop: '20px', padding: '12px', backgroundColor: 'var(--bg-glass)',
              borderRadius: '6px', fontSize: '13px', color: 'var(--text-secondary)',
            }}>
              <strong>{t('settings.hintEnvVars')}</strong>
              <pre style={{
                margin: '8px 0 0', padding: '8px', backgroundColor: 'var(--bg-glass)',
                borderRadius: '4px', fontSize: '12px', overflow: 'auto',
              }}>
{`export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export DEEPSEEK_API_KEY="sk-..."`}
              </pre>
            </div>
          </div>
        )}

        {/* ---- 外观（语言 + 主题） ---- */}
        {activeSection === 'appearance' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionTheme')} / {t('settings.sectionLanguage')}</h1>
            <LanguageSettings />
            <ThemeSettings />
          </div>
        )}

        {/* ---- 搜索引擎 ---- */}
        {activeSection === 'search' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionSearch')}</h1>
            <SearchSettings />
          </div>
        )}

        {/* ---- 心跳自治 ---- */}
        {activeSection === 'heartbeat' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionHeartbeat')}</h1>
            <HeartbeatSettings />
          </div>
        )}

        {/* ---- 备份恢复 ---- */}
        {activeSection === 'backup' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionBackup') || 'Backup'}</h1>
            <BackupSettings />
          </div>
        )}

        {/* ---- 网关（云端连接） ---- */}
        {activeSection === 'gateway' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionCloud')}</h1>
            <AdvancedSettings initialSection="gateway" />
          </div>
        )}

        {/* ---- 嵌入模型 ---- */}
        {activeSection === 'embedding' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionEmbedding')}</h1>
            <AdvancedSettings initialSection="embedding" />
          </div>
        )}

        {/* ---- 语音合成 ---- */}
        {activeSection === 'tts' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>语音合成 (TTS)</h1>
            <TtsSection />
          </div>
        )}

        {activeSection === 'background' && (
          <div style={{ maxWidth: 700 }}>
            <h1 style={{ marginTop: 0, marginBottom: 16 }}>{t('settings.sectionBackground')}</h1>
            <BackgroundModelSection />
          </div>
        )}
      </div>

      {/* 删除确认弹窗（全局浮层） */}
      {deleteConfirm && (
        <div style={{
          position: 'fixed', inset: 0, backgroundColor: 'rgba(0,0,0,0.4)',
          display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 1000,
        }}>
          <div style={{
            backgroundColor: 'var(--bg-elevated)', borderRadius: 12, padding: 24,
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
                  backgroundColor: 'var(--bg-elevated)', cursor: 'pointer', fontSize: 13,
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
    </div>
  )
}

/** 个人资料设置 */
function ProfileSection() {
  const { t } = useI18n()
  const { setProfile } = useAuthStore()
  const [nickname, setNickname] = useState('')
  const [bio, setBio] = useState('')
  const [avatarPreview, setAvatarPreview] = useState<string | null>(null)
  const [avatarBase64, setAvatarBase64] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // 加载个人资料
  useEffect(() => {
    ;(async () => {
      try {
        const profile = await invoke<{ nickname: string; bio: string }>('get_user_profile')
        if (profile) {
          setNickname(profile.nickname || '')
          setBio(profile.bio || '')
        }
      } catch { /* 忽略 */ }
      try {
        const avatar = await invoke<string | null>('get_user_avatar')
        if (avatar) {
          setAvatarPreview(`data:image/png;base64,${avatar}`)
        }
      } catch { /* 忽略 */ }
    })()
  }, [])

  // 头像上传处理：读取文件并缩放到 256x256
  const handleAvatarChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = () => {
      const img = new Image()
      img.onload = () => {
        const canvas = document.createElement('canvas')
        canvas.width = 256
        canvas.height = 256
        const ctx = canvas.getContext('2d')!
        // 居中裁剪
        const size = Math.min(img.width, img.height)
        const sx = (img.width - size) / 2
        const sy = (img.height - size) / 2
        ctx.drawImage(img, sx, sy, size, size, 0, 0, 256, 256)
        const dataUrl = canvas.toDataURL('image/png')
        setAvatarPreview(dataUrl)
        // 提取纯 base64 部分
        setAvatarBase64(dataUrl.replace(/^data:image\/\w+;base64,/, ''))
      }
      img.src = reader.result as string
    }
    reader.readAsDataURL(file)
  }, [])

  const handleSave = async () => {
    setSaving(true)
    try {
      if (avatarBase64) {
        await invoke('save_user_avatar', { base64Data: avatarBase64 })
      }
      await invoke('save_user_profile', { nickname, bio })
      // 更新全局状态，让侧边栏等组件立即显示最新资料
      setProfile({
        nickname,
        bio,
        avatarUrl: avatarPreview || '',
      })
      toast.success(t('profile.saved'))
    } catch (err) {
      toast.error(t('profile.saveFailed') + ': ' + String(err))
    }
    setSaving(false)
  }

  const inputStyle: React.CSSProperties = {
    width: '100%', padding: '10px 12px', borderRadius: 8,
    border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)',
    color: 'var(--text-primary)', fontSize: 14, outline: 'none',
    boxSizing: 'border-box',
  }

  const labelStyle: React.CSSProperties = {
    display: 'block', marginBottom: 6, fontSize: 13,
    fontWeight: 600, color: 'var(--text-secondary)',
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      {/* 头像 */}
      <div>
        <label style={labelStyle}>{t('profile.avatar')}</label>
        <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
          <div
            style={{
              width: 80, height: 80, borderRadius: '50%', overflow: 'hidden',
              backgroundColor: 'var(--bg-elevated)', border: '2px solid var(--border-subtle)',
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              fontSize: 28, color: 'var(--text-muted)',
            }}
          >
            {avatarPreview ? (
              <img src={avatarPreview} alt="avatar" style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
            ) : (
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2" />
                <circle cx="12" cy="7" r="4" />
              </svg>
            )}
          </div>
          <button
            onClick={() => fileInputRef.current?.click()}
            style={{
              padding: '8px 16px', border: '1px solid var(--border-subtle)', borderRadius: 8,
              backgroundColor: 'var(--bg-elevated)', color: 'var(--text-primary)',
              cursor: 'pointer', fontSize: 13,
            }}
          >
            {t('profile.changeAvatar')}
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            onChange={handleAvatarChange}
            style={{ display: 'none' }}
          />
        </div>
      </div>

      {/* 昵称 */}
      <div>
        <label style={labelStyle}>{t('profile.nickname')}</label>
        <input
          type="text"
          value={nickname}
          onChange={(e) => setNickname(e.target.value)}
          placeholder={t('profile.nicknamePlaceholder')}
          maxLength={30}
          style={inputStyle}
        />
      </div>

      {/* 个人简介 */}
      <div>
        <label style={labelStyle}>{t('profile.bio')}</label>
        <textarea
          value={bio}
          onChange={(e) => setBio(e.target.value)}
          placeholder={t('profile.bioPlaceholder')}
          maxLength={200}
          rows={3}
          style={{ ...inputStyle, resize: 'vertical', fontFamily: 'inherit' }}
        />
      </div>

      {/* 保存按钮 */}
      <div>
        <button
          onClick={handleSave}
          disabled={saving}
          style={{
            padding: '10px 24px', border: 'none', borderRadius: 8,
            backgroundColor: 'var(--accent)', color: 'white',
            cursor: saving ? 'not-allowed' : 'pointer', fontSize: 14, fontWeight: 600,
            opacity: saving ? 0.6 : 1,
          }}
        >
          {saving ? t('common.saving') : t('common.save')}
        </button>
      </div>

      {/* 修改密码 */}
      <ChangePasswordSection />
    </div>
  )
}

/** 修改密码区域 */
function ChangePasswordSection() {
  const { t } = useI18n()
  const { user } = useAuthStore()
  const [oldPw, setOldPw] = useState('')
  const [newPw, setNewPw] = useState('')
  const [confirmPw, setConfirmPw] = useState('')
  const [saving, setSaving] = useState(false)
  const [msg, setMsg] = useState<{ type: 'ok' | 'err'; text: string } | null>(null)

  const handleChange = async () => {
    if (!newPw || newPw.length < 6) { setMsg({ type: 'err', text: t('profile.pwTooShort') }); return }
    if (newPw !== confirmPw) { setMsg({ type: 'err', text: t('profile.pwMismatch') }); return }
    setSaving(true); setMsg(null)
    try {
      const { authAPI } = await import('../api/auth')
      const email = user?.email
      if (!email) { setMsg({ type: 'err', text: '未获取到邮箱' }); return }
      // 先验证旧密码（尝试登录）
      if (oldPw) {
        try {
          await authAPI.login('001', email, oldPw)
        } catch {
          setMsg({ type: 'err', text: t('profile.oldPwWrong') }); return
        }
      }
      await authAPI.setPassword(email, newPw)
      setMsg({ type: 'ok', text: t('profile.pwChanged') })
      setOldPw(''); setNewPw(''); setConfirmPw('')
    } catch (e: any) {
      setMsg({ type: 'err', text: e.response?.data?.error || e.message })
    } finally { setSaving(false) }
  }

  const inputStyle = {
    width: '100%', padding: '10px 12px', fontSize: 14,
    border: '1px solid var(--border-subtle)', borderRadius: 8,
    backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
    outline: 'none', boxSizing: 'border-box' as const,
  }
  const labelStyle = { display: 'block', fontSize: 13, fontWeight: 500, color: 'var(--text-secondary)', marginBottom: 6 }

  return (
    <div style={{ marginTop: 32, paddingTop: 24, borderTop: '1px solid var(--border-subtle)' }}>
      <h3 style={{ margin: '0 0 16px', fontSize: 18, fontWeight: 700, color: 'var(--text-primary)' }}>
        {t('profile.changePassword')}
      </h3>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 400 }}>
        <div>
          <label style={labelStyle}>{t('profile.oldPassword')}</label>
          <input type="password" value={oldPw} onChange={e => setOldPw(e.target.value)}
            placeholder={t('profile.oldPwPlaceholder')} style={inputStyle} />
        </div>
        <div>
          <label style={labelStyle}>{t('profile.newPassword')}</label>
          <input type="password" value={newPw} onChange={e => setNewPw(e.target.value)}
            placeholder={t('profile.newPwPlaceholder')} style={inputStyle} />
        </div>
        <div>
          <label style={labelStyle}>{t('profile.confirmPassword')}</label>
          <input type="password" value={confirmPw} onChange={e => setConfirmPw(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) handleChange() }}
            placeholder={t('profile.confirmPwPlaceholder')} style={inputStyle} />
        </div>
        {msg && (
          <div style={{ fontSize: 13, color: msg.type === 'ok' ? 'var(--accent)' : 'var(--error, #ef4444)' }}>
            {msg.text}
          </div>
        )}
        <div>
          <button onClick={handleChange} disabled={saving} style={{
            padding: '10px 24px', border: 'none', borderRadius: 8,
            backgroundColor: 'var(--accent)', color: 'white',
            cursor: saving ? 'not-allowed' : 'pointer', fontSize: 14, fontWeight: 600,
            opacity: saving ? 0.6 : 1,
          }}>
            {saving ? t('common.saving') : t('profile.changePasswordBtn')}
          </button>
        </div>
      </div>
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
        try { setConfig(prev => ({ ...prev, ...JSON.parse(json) })) } catch (e) { console.error('parseHeartbeatConfig failed:', e) }
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
    } catch (e) { toast.error(friendlyError(e)) }
  }

  if (!loaded) return null

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {t('settings.sectionHeartbeat')}
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
            <Select
              value={String(config.interval_secs)}
              onChange={v => save({ interval_secs: Number(v) })}
              options={[
                { value: '600', label: '10 min' },
                { value: '1800', label: '30 min' },
                { value: '3600', label: '1 hour' },
                { value: '7200', label: '2 hours' },
              ]}
              style={{ minWidth: 120 }}
            />
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

/** 搜索引擎设置 */
function SearchSettings() {
  const { t } = useI18n()
  const [provider, setProvider] = useState('auto')
  const [loaded, setLoaded] = useState(false)
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({})
  const [savingKey, setSavingKey] = useState('')

  useEffect(() => {
    invoke<string>('get_setting', { key: 'web_search_provider' }).then(v => {
      if (v) setProvider(v)
      setLoaded(true)
    }).catch(() => setLoaded(true))
    // 加载已配置的 API Key（只显示是否已配置，不显示明文）
    const keyNames = ['BRAVE_API_KEY', 'SERPER_API_KEY', 'EXA_API_KEY', 'TAVILY_API_KEY', 'FIRECRAWL_API_KEY']
    keyNames.forEach(k => {
      invoke<string>('get_setting', { key: `plugin_key_${k}` }).then(v => {
        if (v) setApiKeys(prev => ({ ...prev, [k]: v }))
      }).catch(() => {})
    })
  }, [])

  const save = async (v: string) => {
    setProvider(v)
    try {
      await invoke('set_setting', { key: 'web_search_provider', value: v })
      toast.success(t('common.saved'))
    } catch (e) { toast.error(friendlyError(e)) }
  }

  const saveApiKey = async (keyName: string, value: string) => {
    setSavingKey(keyName)
    try {
      await invoke('set_setting', { key: `plugin_key_${keyName}`, value })
      setApiKeys(prev => ({ ...prev, [keyName]: value }))
      toast.success(`${keyName} ${t('common.saved')}`)
    } catch (e) { toast.error(friendlyError(e)) }
    finally { setSavingKey('') }
  }

  if (!loaded) return null

  const options: Array<{ value: string; label: string; desc: string; keyName?: string; link?: string }> = [
    { value: 'auto', label: t('settings.searchAuto'), desc: 'Brave → Serper → Exa → Tavily → Firecrawl → DuckDuckGo' },
    { value: 'brave', label: 'Brave Search', keyName: 'BRAVE_API_KEY', link: 'https://brave.com/search/api/', desc: t('settings.searchNeedsKey') },
    { value: 'serper', label: 'Serper (Google)', keyName: 'SERPER_API_KEY', link: 'https://serper.dev/', desc: t('settings.searchNeedsKey') + ' — 2500 ' + t('settings.searchFree') + '/mo' },
    { value: 'exa', label: 'Exa (Neural)', keyName: 'EXA_API_KEY', link: 'https://exa.ai/', desc: t('settings.searchNeedsKey') },
    { value: 'tavily', label: 'Tavily AI', keyName: 'TAVILY_API_KEY', link: 'https://tavily.com/', desc: t('settings.searchNeedsKey') + ' — 1000 ' + t('settings.searchFree') + '/mo' },
    { value: 'firecrawl', label: 'Firecrawl', keyName: 'FIRECRAWL_API_KEY', link: 'https://firecrawl.dev/', desc: t('settings.searchNeedsKey') },
    { value: 'duckduckgo', label: 'DuckDuckGo', desc: t('settings.searchFree') },
  ]

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {t('settings.sectionSearch')}
      </h3>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {options.map(opt => (
          <div key={opt.value} style={{
            padding: '8px 12px', borderRadius: 8,
            border: provider === opt.value ? '2px solid var(--accent)' : '1px solid var(--border-subtle)',
            backgroundColor: provider === opt.value ? 'var(--accent-bg)' : 'transparent',
          }}>
            <label style={{ display: 'flex', alignItems: 'center', gap: 10, cursor: 'pointer' }}>
              <input type="radio" name="search" checked={provider === opt.value}
                onChange={() => save(opt.value)} style={{ margin: 0 }} />
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: provider === opt.value ? 600 : 400 }}>
                  {opt.label}
                  {opt.keyName && apiKeys[opt.keyName] && (
                    <span style={{ marginLeft: 8, fontSize: 10, color: 'var(--success)', fontWeight: 500 }}>
                      &#x2713; {t('settings.keyConfigured') || 'Key configured'}
                    </span>
                  )}
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>{opt.desc}</div>
              </div>
            </label>
            {/* API Key 输入框：选中且需要 key 时显示 */}
            {provider === opt.value && opt.keyName && (
              <div style={{ marginTop: 8, marginLeft: 24, display: 'flex', gap: 6, alignItems: 'center' }}>
                <input
                  type="password"
                  placeholder={`${opt.keyName}`}
                  defaultValue={apiKeys[opt.keyName!] || ''}
                  onBlur={(e) => {
                    const v = e.target.value.trim()
                    if (v && v !== apiKeys[opt.keyName!]) saveApiKey(opt.keyName!, v)
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) {
                      const v = (e.target as HTMLInputElement).value.trim()
                      if (v) saveApiKey(opt.keyName!, v)
                    }
                  }}
                  style={{
                    flex: 1, padding: '6px 10px', fontSize: 12, borderRadius: 6,
                    border: '1px solid var(--border-default)', backgroundColor: 'var(--bg-glass)',
                    color: 'var(--text-primary)', outline: 'none', fontFamily: "'SF Mono', Monaco, monospace",
                  }}
                />
                {opt.link && (
                  <a href={opt.link} target="_blank" rel="noopener noreferrer"
                    style={{ fontSize: 11, color: 'var(--accent)', whiteSpace: 'nowrap', textDecoration: 'none' }}
                    onClick={(e) => { e.preventDefault(); invoke('open_url', { url: opt.link }).catch(() => window.open(opt.link, '_blank')) }}
                  >
                    {t('settings.getKey') || 'Get Key'} &#x2197;
                  </a>
                )}
                {savingKey === opt.keyName && <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{t('common.saving')}</span>}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

/** 数据备份设置 */
function BackupSettings() {
  const { t } = useI18n()
  const [backupResult, setBackupResult] = useState<string>('')
  const [backing, setBacking] = useState(false)

  const handleBackup = async () => {
    setBacking(true)
    try {
      const result = await invoke<string>('backup_database')
      const parsed = JSON.parse(result)
      setBackupResult(`Backup saved: ${parsed.path} (${(parsed.size_bytes / 1024 / 1024).toFixed(1)} MB)`)
      toast.success('Backup complete')
    } catch (e) { toast.error(friendlyError(e)) }
    finally { setBacking(false) }
  }

  const handleRestore = () => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.db'
    input.onchange = async () => {
      if (!input.files?.[0]) return
      // Tauri 环境下需要用文件路径
      toast.error('Please use the file path directly. Drag the backup .db file here is not supported yet — use CLI or manual copy.')
    }
    input.click()
  }

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {t('settings.sectionBackup') || 'Data Backup'}
      </h3>
      <p style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12 }}>
        {t('settings.backupDesc') || 'Create a consistent backup of all data (conversations, agents, settings, memory).'}
      </p>
      <div style={{ display: 'flex', gap: 8 }}>
        <button onClick={handleBackup} disabled={backing} style={{
          padding: '8px 16px', backgroundColor: 'var(--accent)', color: '#fff',
          border: 'none', borderRadius: 6, cursor: backing ? 'wait' : 'pointer', fontSize: 13,
        }}>
          {backing ? 'Backing up...' : (t('settings.backupBtn') || 'Backup Now')}
        </button>
      </div>
      {backupResult && (
        <div style={{ marginTop: 8, fontSize: 12, color: 'var(--success)', fontFamily: 'monospace' }}>
          {backupResult}
        </div>
      )}
    </div>
  )
}

/** 高级设置面板（嵌入配置 + 系统状态 + 缓存统计） */
function AdvancedSettings({ initialSection }: { initialSection?: 'gateway' | 'embedding' } = {}) {
  const { t } = useI18n()
  const [expanded, setExpanded] = useState(!!initialSection)
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

  useEffect(() => { loadSettings() }, [])

  const showEmbedding = !initialSection || initialSection === 'embedding'
  const showGateway = !initialSection || initialSection === 'gateway'

  return (
    <div style={{ padding: '16px', border: '1px solid var(--border-subtle)', borderRadius: '8px' }}>
      {/* 向量嵌入 */}
      {showEmbedding && (
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
      )}

      {/* 云端连接（混合架构） */}
      {showGateway && (
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
      )}

      {/* Token 限额 */}
      {showGateway && (
        <div style={{ marginBottom: '16px' }}>
          <h4 style={{ margin: '0 0 8px', color: 'var(--text-secondary)' }}>{t('settings.sectionDailyLimit')}</h4>
          <input placeholder={t('settings.fieldDailyLimit')} value={dailyLimit} onChange={e => setDailyLimit(e.target.value)}
            style={{ width: '200px', padding: '6px' }} type="number" />
        </div>
      )}

      <button onClick={saveSettings} disabled={saving} style={{
        padding: '8px 20px', backgroundColor: 'var(--success)', color: 'white',
        border: 'none', borderRadius: '4px', cursor: saving ? 'not-allowed' : 'pointer',
        marginBottom: '16px',
      }}>
        {saving ? t('common.saving') : t('common.save')}
      </button>

      {/* 系统状态 */}
      {health && (
        <div style={{ marginTop: '12px', padding: '12px', backgroundColor: 'var(--bg-glass)', borderRadius: '6px', fontSize: '13px' }}>
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
        {t('settings.sectionLanguage')}
      </h3>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{t('settings.labelLanguage')}</span>
        <Select
          value={locale}
          onChange={v => setLocale(v as Locale)}
          options={SUPPORTED_LOCALES.map(loc => ({ value: loc, label: LOCALE_LABELS[loc] }))}
          style={{ minWidth: 140 }}
        />
      </div>
    </div>
  )
}

function ThemeSettings() {
  const { t } = useI18n()
  const { theme, setTheme } = useTheme()

  const themes: { value: Theme; label: string; icon: string }[] = [
    { value: 'light', label: t('settings.themeLight'), icon: '' },
    { value: 'dark', label: t('settings.themeDark'), icon: '' },
    { value: 'system', label: t('settings.themeSystem'), icon: '' },
  ]

  return (
    <div style={{ marginTop: 24, padding: '16px 20px', borderRadius: 12, border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-elevated)' }}>
      <h3 style={{ margin: '0 0 12px', fontSize: 15, fontWeight: 600 }}>
        {t('settings.sectionTheme')}
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

/** 后台模型配置 — 经验提取、上下文压缩、定时任务等使用的模型 */
function TtsSection() {
  const [provider, setProvider] = useState('local')
  const [apiKey, setApiKey] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [model, setModel] = useState('')
  const [defaultVoice, setDefaultVoice] = useState('')
  const [defaultStyle, setDefaultStyle] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    ;(async () => {
      try {
        const keys = ['tts.provider', 'tts.api_key', 'tts.base_url', 'tts.model', 'tts.default_voice', 'tts.default_style']
        for (const key of keys) {
          const val = await invoke<string | null>('get_setting', { key })
          if (!val) continue
          if (key === 'tts.provider') setProvider(val)
          if (key === 'tts.api_key') setApiKey(val)
          if (key === 'tts.base_url') setBaseUrl(val)
          if (key === 'tts.model') setModel(val)
          if (key === 'tts.default_voice') setDefaultVoice(val)
          if (key === 'tts.default_style') setDefaultStyle(val)
        }
      } catch {}
    })()
  }, [])

  const save = async () => {
    setSaving(true)
    try {
      await invoke('set_setting', { key: 'tts.provider', value: provider })
      await invoke('set_setting', { key: 'tts.api_key', value: apiKey })
      await invoke('set_setting', { key: 'tts.base_url', value: baseUrl })
      await invoke('set_setting', { key: 'tts.model', value: model })
      await invoke('set_setting', { key: 'tts.default_voice', value: defaultVoice })
      await invoke('set_setting', { key: 'tts.default_style', value: defaultStyle })
      toast.success('TTS 配置已保存')
    } catch (e) { toast.error(friendlyError(e)) }
    setSaving(false)
  }

  const inputStyle: React.CSSProperties = { width: '100%', padding: '8px 12px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 14, background: 'var(--bg-secondary)' }
  const labelStyle: React.CSSProperties = { display: 'block', fontSize: 13, fontWeight: 500, marginBottom: 4, color: 'var(--text-secondary)' }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div>
        <label style={labelStyle}>TTS 引擎</label>
        <select value={provider} onChange={e => setProvider(e.target.value)} style={{ ...inputStyle, cursor: 'pointer' }}>
          <option value="local">本地系统 TTS（免费，无需配置）</option>
          <option value="mimo">小米 MiMo-V2-TTS（高质量，免费）</option>
          <option value="openai">OpenAI TTS（需 API Key）</option>
        </select>
      </div>

      {provider !== 'local' && (
        <>
          <div>
            <label style={labelStyle}>API Key</label>
            <input type="password" value={apiKey} onChange={e => setApiKey(e.target.value)}
              placeholder={provider === 'mimo' ? '小米 MiMo API Key' : 'OpenAI API Key'}
              style={inputStyle} />
          </div>
          <div>
            <label style={labelStyle}>API 地址</label>
            <input value={baseUrl} onChange={e => setBaseUrl(e.target.value)}
              placeholder={provider === 'mimo' ? 'https://token-plan-cn.xiaomimimo.com/v1' : 'https://api.openai.com/v1'}
              style={inputStyle} />
            {provider === 'mimo' && !baseUrl && (
              <span style={{ fontSize: 11, color: 'var(--text-muted)', cursor: 'pointer' }}
                onClick={() => setBaseUrl('https://token-plan-cn.xiaomimimo.com/v1')}>
                点击填入默认地址
              </span>
            )}
          </div>
          <div>
            <label style={labelStyle}>模型名称</label>
            <input value={model} onChange={e => setModel(e.target.value)}
              placeholder={provider === 'mimo' ? 'mimo-v2-tts' : 'tts-1'}
              style={inputStyle} />
          </div>
        </>
      )}

      {provider === 'mimo' && (
        <div>
          <label style={labelStyle}>默认语音风格</label>
          <input value={defaultStyle} onChange={e => setDefaultStyle(e.target.value)}
            placeholder="如：温柔的女声、东北口音、播音员、唱歌"
            style={inputStyle} />
          <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
            MiMo-V2-TTS 支持自然语言风格控制，留空使用默认声音
          </span>
        </div>
      )}

      {provider === 'openai' && (
        <div>
          <label style={labelStyle}>默认声音</label>
          <select value={defaultVoice} onChange={e => setDefaultVoice(e.target.value)} style={{ ...inputStyle, cursor: 'pointer' }}>
            <option value="">默认 (alloy)</option>
            <option value="alloy">Alloy</option>
            <option value="echo">Echo</option>
            <option value="fable">Fable</option>
            <option value="onyx">Onyx</option>
            <option value="nova">Nova</option>
            <option value="shimmer">Shimmer</option>
          </select>
        </div>
      )}

      <button onClick={save} disabled={saving}
        style={{ alignSelf: 'flex-start', padding: '8px 24px', borderRadius: 6, border: 'none', background: 'var(--accent)', color: '#fff', cursor: 'pointer', fontSize: 14 }}>
        {saving ? '保存中...' : '保存'}
      </button>
    </div>
  )
}

function BackgroundModelSection() {
  const { t } = useI18n()
  const [bgModel, setBgModel] = useState('')
  const [saving, setSaving] = useState(false)
  const [msg, setMsg] = useState('')

  useEffect(() => {
    invoke<string>('get_setting', { key: 'background_model' }).then(v => setBgModel(v || '')).catch(() => {})
  }, [])

  const handleSave = async () => {
    setSaving(true)
    setMsg('')
    try {
      await invoke('set_setting', { key: 'background_model', value: bgModel })
      setMsg(t('settings.successSaved'))
    } catch (e) { setMsg(String(e)) }
    finally { setSaving(false) }
  }

  return (
    <div>
      <p style={{ color: 'var(--text-secondary)', fontSize: 13, marginBottom: 20 }}>
        {t('settings.backgroundDesc')}
      </p>

      <div style={{ marginBottom: 20 }}>
        <label style={{ display: 'block', fontSize: 13, fontWeight: 500, marginBottom: 6, color: 'var(--text-secondary)' }}>
          {t('settings.backgroundModel')}
        </label>
        <ProviderModelSelector value={bgModel} onChange={setBgModel} requireKey={false} />
        <span style={{ fontSize: 11, color: 'var(--text-muted)', display: 'block', marginTop: 4 }}>
          {t('settings.backgroundModelHint')}
        </span>
      </div>

      {msg && <p style={{ color: msg.includes('成功') || msg.includes('success') ? 'var(--accent)' : 'var(--error)', fontSize: 13, marginBottom: 12 }}>{msg}</p>}

      <button
        onClick={handleSave}
        disabled={saving}
        style={{
          padding: '10px 24px', border: 'none', borderRadius: 8,
          backgroundColor: 'var(--accent)', color: 'white', cursor: 'pointer',
          fontSize: 14, fontWeight: 600, opacity: saving ? 0.6 : 1,
        }}
      >
        {saving ? '...' : t('settings.save')}
      </button>
    </div>
  )
}
