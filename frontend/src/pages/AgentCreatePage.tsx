/**
 * Agent 创建向导页面
 *
 * 三步向导：基本信息 → 模型配置 → 预览 & 创建
 * 挂载路由：/agents/new
 */

import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useNavigate } from 'react-router-dom'
import { useI18n } from '../i18n'

interface ProviderModel {
  id: string
  name: string
}

interface Provider {
  id: string
  name: string
  baseUrl: string
  apiKeyMasked?: string
  models: ProviderModel[]
  enabled: boolean
}

interface ModelItem {
  id: string
  label: string
  provider: string
  providerName: string
  available: boolean
}

interface Agent {
  id: string
  name: string
  model: string
  systemPrompt: string
  createdAt: number
}

/** 预置 Agent 模板 */
const TEMPLATES = [
  { nameKey: 'chatPage.templateGeneral', prompt: '你是一个有用的AI助手，擅长回答各种问题。', icon: '💬' },
  { nameKey: 'chatPage.templateCoding', prompt: '你是一个资深编程助手，擅长代码编写、调试和架构设计。请用简洁专业的方式回答。', icon: '👨‍💻' },
  { nameKey: 'chatPage.templateTranslator', prompt: '你是一个专业翻译，擅长中英互译。保持原文风格和语气，翻译要自然流畅。', icon: '🌐' },
  { nameKey: 'chatPage.templateWriter', prompt: '你是一个专业写作助手，擅长文章撰写、润色和创意写作。', icon: '✍️' },
]

/** 温度预设 */
const TEMP_PRESETS = [
  { id: 'precise', labelKey: 'agentCreate.tempPrecise', value: 0.2 },
  { id: 'balanced', labelKey: 'agentCreate.tempBalanced', value: 0.7 },
  { id: 'creative', labelKey: 'agentCreate.tempCreative', value: 1.2 },
]

const STEP_KEYS = ['agentCreate.stepBasic', 'agentCreate.stepModel', 'agentCreate.stepPreview']

export default function AgentCreatePage() {
  const navigate = useNavigate()
  const { t } = useI18n()

  // 创建模式：manual | ai
  const [mode, setMode] = useState<'manual' | 'ai'>('manual')
  const [aiDescription, setAiDescription] = useState('')
  const [aiGenerating, setAiGenerating] = useState(false)

  // 步骤状态
  const [step, setStep] = useState(0)

  // Step 1: 基本信息
  const [name, setName] = useState('')
  const [systemPrompt, setSystemPrompt] = useState('你是一个有用的AI助手。')

  // Step 2: 模型配置
  const [allModels, setAllModels] = useState<ModelItem[]>([])
  const [selectedModel, setSelectedModel] = useState('')
  const [temperature, setTemperature] = useState(0.7)
  const [maxTokens, setMaxTokens] = useState(2048)

  // 全局状态
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')

  // 加载供应商和模型列表
  useEffect(() => {
    const load = async () => {
      try {
        const providers = await invoke<Provider[]>('get_providers')
        const models: ModelItem[] = []
        for (const p of providers || []) {
          if (!p.enabled) continue
          const hasKey = !!p.apiKeyMasked && p.apiKeyMasked !== ''
          const isLocal = p.id === 'ollama' || p.baseUrl?.includes('localhost')
          const available = hasKey || isLocal
          for (const m of p.models || []) {
            models.push({
              id: m.id,
              label: m.name,
              provider: p.id,
              providerName: p.name,
              available,
            })
          }
        }
        setAllModels(models)
        const first = models.find((m) => m.available)
        if (first) setSelectedModel(first.id)
      } catch (e) {
        console.error('加载模型列表失败:', e)
      }
    }
    load()
  }, [])

  // 应用模板
  const applyTemplate = (tpl: (typeof TEMPLATES)[0]) => {
    setName(t(tpl.nameKey))
    setSystemPrompt(tpl.prompt)
  }

  // AI 生成 Agent 配置
  const handleAiGenerate = async () => {
    if (!aiDescription.trim()) return
    setAiGenerating(true)
    setError('')
    try {
      const config = await invoke<{
        name: string
        systemPrompt: string
        model: string
        temperature: number
        maxTokens: number
      }>('ai_generate_agent_config', { description: aiDescription.trim() })

      // 填充表单
      setName(config.name || '')
      setSystemPrompt(config.systemPrompt || '')
      if (config.model && allModels.some((m) => m.id === config.model)) {
        setSelectedModel(config.model)
      }
      if (config.temperature != null) setTemperature(config.temperature)
      if (config.maxTokens != null) setMaxTokens(config.maxTokens)

      // 跳到预览步骤
      setMode('manual')
      setStep(2)
    } catch (e: any) {
      setError(String(e?.message || e || t('agentCreate.aiGenerateFailed')))
    } finally {
      setAiGenerating(false)
    }
  }

  // 温度预设匹配
  const activePreset = TEMP_PRESETS.find((p) => Math.abs(p.value - temperature) < 0.05)
  const STEPS = STEP_KEYS.map(k => t(k))

  // 当前选中模型的信息
  const modelInfo = allModels.find((m) => m.id === selectedModel)

  // 步骤校验
  const canNext = (): boolean => {
    if (step === 0) return name.trim().length > 0 && systemPrompt.trim().length > 0
    if (step === 1) return !!selectedModel && (modelInfo?.available ?? false)
    return true
  }

  // 创建 Agent
  const handleCreate = async () => {
    setCreating(true)
    setError('')
    try {
      const agent = await invoke<Agent>('create_agent', {
        name: name.trim(),
        systemPrompt: systemPrompt.trim(),
        model: selectedModel,
      })
      if (agent?.id) {
        // 设置温度和 maxTokens
        await invoke('update_agent', {
          agentId: agent.id,
          model: selectedModel,
          temperature,
          maxTokens,
        })
        navigate(`/agents/${agent.id}`)
      }
    } catch (e: any) {
      setError(String(e?.message || e || t('chatPage.createFailed')))
      setCreating(false)
    }
  }

  // ── 渲染 ──

  return (
    <div style={{ maxWidth: 640, margin: '0 auto', padding: '32px 20px' }}>
      {/* 模式切换 */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 24 }}>
        {(['manual', 'ai'] as const).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            style={{
              padding: '8px 20px', borderRadius: 8, cursor: 'pointer', fontSize: 14, fontWeight: 500,
              border: mode === m ? '2px solid #007bff' : '2px solid #e5e7eb',
              backgroundColor: mode === m ? '#e8f0fe' : 'white',
              color: mode === m ? '#007bff' : '#666',
            }}
          >
            {m === 'manual' ? t('agentCreate.modeManual') : t('agentCreate.modeAi')}
          </button>
        ))}
      </div>

      {/* AI 生成模式 */}
      {mode === 'ai' ? (
        <div>
          <h2 style={{ margin: '0 0 8px', fontSize: 20 }}>{t('agentCreate.aiTitle')}</h2>
          <p style={{ color: 'var(--text-secondary)', fontSize: 14, margin: '0 0 20px' }}>
            {t('agentCreate.aiDesc')}
          </p>
          <textarea
            value={aiDescription}
            onChange={(e) => setAiDescription(e.target.value)}
            placeholder={t('agentCreate.aiPlaceholder')}
            rows={5}
            style={{
              width: '100%', padding: 12, border: '1px solid var(--border-subtle)', borderRadius: 8,
              fontSize: 14, resize: 'vertical', boxSizing: 'border-box',
            }}
          />
          {error && (
            <div style={{ padding: 10, backgroundColor: 'var(--error-bg)', color: '#dc2626', borderRadius: 8, marginTop: 12, fontSize: 13 }}>
              {error}
            </div>
          )}
          <button
            onClick={handleAiGenerate}
            disabled={aiGenerating || !aiDescription.trim()}
            style={{
              marginTop: 16, padding: '12px 32px', backgroundColor: 'var(--accent)', color: 'white',
              border: 'none', borderRadius: 8, fontSize: 15, fontWeight: 500, cursor: 'pointer',
              opacity: aiGenerating || !aiDescription.trim() ? 0.6 : 1,
            }}
          >
            {aiGenerating ? t('agentCreate.aiGenerating') : t('agentCreate.aiSubmit')}
          </button>
        </div>
      ) : (
      <>
      {/* 步骤指示器 */}
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 32 }}>
        {STEPS.map((label, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', flex: i < STEPS.length - 1 ? 1 : undefined }}>
            <div
              style={{
                width: 32,
                height: 32,
                borderRadius: '50%',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 14,
                fontWeight: 600,
                color: i <= step ? '#fff' : '#999',
                backgroundColor: i < step ? '#28a745' : i === step ? '#007bff' : '#e9ecef',
                transition: 'background-color 0.2s',
              }}
            >
              {i < step ? '✓' : i + 1}
            </div>
            <span
              style={{
                marginLeft: 8,
                fontSize: 13,
                fontWeight: i === step ? 600 : 400,
                color: i <= step ? '#333' : '#999',
                whiteSpace: 'nowrap',
              }}
            >
              {label}
            </span>
            {i < STEPS.length - 1 && (
              <div
                style={{
                  flex: 1,
                  height: 2,
                  margin: '0 12px',
                  backgroundColor: i < step ? '#28a745' : '#e9ecef',
                  transition: 'background-color 0.2s',
                }}
              />
            )}
          </div>
        ))}
      </div>

      {/* Step 1: 基本信息 */}
      {step === 0 && (
        <div>
          <h3 style={{ margin: '0 0 20px', fontSize: 18, fontWeight: 600 }}>{t('agentCreate.stepBasic')}</h3>

          {/* 模板选择 */}
          <div style={{ marginBottom: 20 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 8 }}>
              {t('agentCreate.quickTemplates')}
            </label>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
              {TEMPLATES.map((tpl) => (
                <button
                  key={tpl.nameKey}
                  onClick={() => applyTemplate(tpl)}
                  style={{
                    padding: '10px 12px',
                    border: name === t(tpl.nameKey) ? '2px solid #007bff' : '1px solid #ddd',
                    borderRadius: 8,
                    backgroundColor: name === t(tpl.nameKey) ? '#e7f1ff' : '#fff',
                    cursor: 'pointer',
                    textAlign: 'left',
                    fontSize: 13,
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                  }}
                >
                  <span style={{ fontSize: 20 }}>{tpl.icon}</span>
                  <span>{t(tpl.nameKey)}</span>
                </button>
              ))}
            </div>
          </div>

          {/* Agent 名称 */}
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldName')} <span style={{ color: '#dc3545' }}>*</span>
            </label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('agentCreate.placeholderName')}
              style={{
                width: '100%',
                padding: '10px 12px',
                border: '1px solid var(--border-subtle)',
                borderRadius: 6,
                fontSize: 14,
                boxSizing: 'border-box',
              }}
            />
          </div>

          {/* 系统提示词 */}
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldSystemPrompt')} <span style={{ color: '#dc3545' }}>*</span>
            </label>
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              rows={5}
              placeholder={t('agentCreate.placeholderSystemPrompt')}
              style={{
                width: '100%',
                padding: '10px 12px',
                border: '1px solid var(--border-subtle)',
                borderRadius: 6,
                fontSize: 14,
                resize: 'vertical',
                fontFamily: 'inherit',
                boxSizing: 'border-box',
              }}
            />
          </div>
        </div>
      )}

      {/* Step 2: 模型配置 */}
      {step === 1 && (
        <div>
          <h3 style={{ margin: '0 0 20px', fontSize: 18, fontWeight: 600 }}>{t('agentCreate.stepModel')}</h3>

          {/* 模型选择 */}
          <div style={{ marginBottom: 20 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldModel')}
            </label>
            {allModels.length === 0 ? (
              <div style={{ padding: 12, backgroundColor: '#fff3cd', borderRadius: 6, fontSize: 13, color: '#856404' }}>
                {t('agentCreate.warningNoModels')}
              </div>
            ) : (
              <select
                value={selectedModel}
                onChange={(e) => setSelectedModel(e.target.value)}
                style={{
                  width: '100%',
                  padding: '10px 12px',
                  border: '1px solid var(--border-subtle)',
                  borderRadius: 6,
                  fontSize: 14,
                  backgroundColor: 'var(--bg-elevated)',
                  boxSizing: 'border-box',
                }}
              >
                {allModels.map((m) => (
                  <option key={m.id} value={m.id} disabled={!m.available}>
                    {m.label} ({m.providerName}){!m.available ? ` — ${t('settings.labelNoKey')}` : ''}
                  </option>
                ))}
              </select>
            )}
          </div>

          {/* 温度 */}
          <div style={{ marginBottom: 20 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldTemperature')}: {temperature.toFixed(1)}
            </label>
            <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
              {TEMP_PRESETS.map((p) => (
                <button
                  key={p.id}
                  onClick={() => setTemperature(p.value)}
                  style={{
                    padding: '4px 14px',
                    fontSize: 12,
                    border: '1px solid',
                    borderColor: activePreset?.id === p.id ? '#007bff' : '#ddd',
                    borderRadius: 4,
                    backgroundColor: activePreset?.id === p.id ? '#e7f1ff' : '#fff',
                    color: activePreset?.id === p.id ? '#007bff' : '#333',
                    cursor: 'pointer',
                  }}
                >
                  {t(p.labelKey)} ({p.value})
                </button>
              ))}
            </div>
            <input
              type="range"
              min={0}
              max={2}
              step={0.1}
              value={temperature}
              onChange={(e) => setTemperature(parseFloat(e.target.value))}
              style={{ width: '100%' }}
            />
            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-muted)' }}>
              <span>{t('agentCreate.tempPrecise')} 0</span>
              <span>{t('agentCreate.tempCreative')} 2</span>
            </div>
          </div>

          {/* Max Tokens */}
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldMaxTokens')}: {maxTokens}
            </label>
            <input
              type="range"
              min={256}
              max={8192}
              step={256}
              value={maxTokens}
              onChange={(e) => setMaxTokens(parseInt(e.target.value))}
              style={{ width: '100%' }}
            />
            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-muted)' }}>
              <span>256</span>
              <span>8192</span>
            </div>
          </div>
        </div>
      )}

      {/* Step 3: 预览 & 创建 */}
      {step === 2 && (
        <div>
          <h3 style={{ margin: '0 0 20px', fontSize: 18, fontWeight: 600 }}>{t('agentCreate.stepPreview')}</h3>

          <div
            style={{
              border: '1px solid #e9ecef',
              borderRadius: 8,
              overflow: 'hidden',
            }}
          >
            {/* 名称 */}
            <div style={{ padding: '12px 16px', borderBottom: '1px solid #e9ecef' }}>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('common.name')}</div>
              <div style={{ fontSize: 15, fontWeight: 600 }}>{name}</div>
            </div>

            {/* 系统提示词 */}
            <div style={{ padding: '12px 16px', borderBottom: '1px solid #e9ecef' }}>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('agentCreate.fieldSystemPrompt')}</div>
              <div
                style={{
                  fontSize: 13,
                  color: 'var(--text-secondary)',
                  whiteSpace: 'pre-wrap',
                  maxHeight: 120,
                  overflowY: 'auto',
                }}
              >
                {systemPrompt}
              </div>
            </div>

            {/* 模型 */}
            <div style={{ padding: '12px 16px', borderBottom: '1px solid #e9ecef' }}>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('common.model')}</div>
              <div style={{ fontSize: 14 }}>
                {modelInfo?.label || selectedModel}
                {modelInfo && (
                  <span style={{ color: 'var(--text-muted)', marginLeft: 6, fontSize: 12 }}>
                    ({modelInfo.providerName})
                  </span>
                )}
              </div>
            </div>

            {/* 参数 */}
            <div style={{ padding: '12px 16px', display: 'flex', gap: 32 }}>
              <div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('agentCreate.fieldTemperature')}</div>
                <div style={{ fontSize: 14 }}>
                  {temperature.toFixed(1)}
                  {activePreset && (
                    <span style={{ color: 'var(--text-muted)', marginLeft: 6, fontSize: 12 }}>
                      ({t(activePreset.labelKey)})
                    </span>
                  )}
                </div>
              </div>
              <div>
                <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('agentCreate.fieldMaxTokens')}</div>
                <div style={{ fontSize: 14 }}>{maxTokens}</div>
              </div>
            </div>
          </div>

          {error && (
            <div
              style={{
                marginTop: 16,
                padding: '10px 14px',
                backgroundColor: '#f8d7da',
                color: '#842029',
                borderRadius: 6,
                fontSize: 13,
              }}
            >
              {error}
            </div>
          )}
        </div>
      )}

      {/* 底部导航按钮 */}
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          marginTop: 32,
          paddingTop: 20,
          borderTop: '1px solid #e9ecef',
        }}
      >
        <button
          onClick={() => (step === 0 ? navigate(-1) : setStep(step - 1))}
          disabled={creating}
          style={{
            padding: '10px 24px',
            fontSize: 14,
            border: '1px solid var(--border-subtle)',
            borderRadius: 6,
            backgroundColor: 'var(--bg-elevated)',
            color: 'var(--text-primary)',
            cursor: creating ? 'not-allowed' : 'pointer',
          }}
        >
          {step === 0 ? t('common.cancel') : t('common.prev')}
        </button>

        {step < 2 ? (
          <button
            onClick={() => setStep(step + 1)}
            disabled={!canNext()}
            style={{
              padding: '10px 24px',
              fontSize: 14,
              border: 'none',
              borderRadius: 6,
              backgroundColor: canNext() ? '#007bff' : '#ccc',
              color: '#fff',
              cursor: canNext() ? 'pointer' : 'not-allowed',
              fontWeight: 500,
            }}
          >
            {t('common.next')}
          </button>
        ) : (
          <button
            onClick={handleCreate}
            disabled={creating}
            style={{
              padding: '10px 28px',
              fontSize: 14,
              border: 'none',
              borderRadius: 6,
              backgroundColor: creating ? '#6c757d' : '#28a745',
              color: '#fff',
              cursor: creating ? 'not-allowed' : 'pointer',
              fontWeight: 600,
            }}
          >
            {creating ? t('common.creating') : t('common.create')}
          </button>
        )}
      </div>
      </>
      )}
    </div>
  )
}
