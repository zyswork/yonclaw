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
import Select from '../components/Select'

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
  { nameKey: 'chatPage.templateGeneral', name: '通用助理', prompt: '你是一个有用的AI助手，擅长回答各种问题。', icon: 'AI', desc: '日常问答、信息整理' },
  { nameKey: 'chatPage.templateCoding', name: '编程助手', prompt: '你是一个资深编程助手，擅长代码编写、调试和架构设计。请用简洁专业的方式回答。', icon: '<>', desc: '全栈开发、代码调试' },
  { nameKey: 'chatPage.templateTranslator', name: '翻译助手', prompt: '你是一个专业翻译，擅长中英互译。保持原文风格和语气，翻译要自然流畅。', icon: 'Aa', desc: '中英互译、多语言' },
  { nameKey: 'chatPage.templateWriter', name: '写作助手', prompt: '你是一个专业写作助手，擅长文章撰写、润色和创意写作。', icon: 'W', desc: '文案创作、内容润色' },
  { nameKey: '', name: '数据分析师', prompt: '你是一位数据分析专家。擅长数据解读、统计分析、趋势预测。能够处理 CSV/Excel 数据，生成分析报告。', icon: '#', desc: '数据分析、报表解读' },
  { nameKey: '', name: '学习导师', prompt: '你是一位耐心的学习导师。用简单易懂的方式解释复杂概念，善于用类比和例子帮助理解。根据学生水平调整讲解深度。', icon: 'E', desc: '知识讲解、学习指导' },
  { nameKey: '', name: '创意顾问', prompt: '你是一位创意顾问。擅长头脑风暴、创意方案设计、营销策划。善于跳出常规思维，提供新颖独特的视角和解决方案。', icon: '*', desc: '头脑风暴、营销策划' },
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
    } catch (e: unknown) {
      setError(String((e as Error)?.message || e || t('agentCreate.aiGenerateFailed')))
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
    } catch (e: unknown) {
      setError(String((e as Error)?.message || e || t('chatPage.createFailed')))
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
              border: mode === m ? '2px solid var(--accent)' : '2px solid var(--border-subtle)',
              backgroundColor: mode === m ? 'var(--accent-bg)' : 'var(--bg-elevated)',
              color: mode === m ? 'var(--accent)' : 'var(--text-secondary)',
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
            <div style={{ padding: 10, backgroundColor: 'var(--error-bg)', color: 'var(--error)', borderRadius: 8, marginTop: 12, fontSize: 13 }}>
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
                color: i <= step ? '#fff' : 'var(--text-muted)',
                backgroundColor: i < step ? 'var(--success)' : i === step ? 'var(--accent)' : 'var(--bg-glass)',
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
                color: i <= step ? 'var(--text-primary)' : 'var(--text-muted)',
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
                  backgroundColor: i < step ? 'var(--success)' : 'var(--bg-glass)',
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
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))', gap: 8 }}>
              {TEMPLATES.map((tpl, idx) => {
                const tplName = tpl.nameKey ? t(tpl.nameKey) : tpl.name
                const isSelected = name === tplName
                return (
                  <button
                    key={tpl.nameKey || idx}
                    onClick={() => { setName(tplName); setSystemPrompt(tpl.prompt) }}
                    style={{
                      padding: '12px 10px',
                      border: isSelected ? '2px solid var(--accent)' : '1px solid var(--border-subtle)',
                      borderRadius: 10,
                      backgroundColor: isSelected ? 'var(--accent-bg)' : 'var(--bg-elevated)',
                      cursor: 'pointer',
                      textAlign: 'center',
                      fontSize: 12,
                    }}
                  >
                    <div style={{ fontSize: 24, marginBottom: 4 }}>{tpl.icon}</div>
                    <div style={{ fontWeight: 600, fontSize: 13 }}>{tplName}</div>
                    {tpl.desc && <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>{tpl.desc}</div>}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Agent 名称 */}
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 6 }}>
              {t('agentCreate.fieldName')} <span style={{ color: 'var(--error)' }}>*</span>
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
              {t('agentCreate.fieldSystemPrompt')} <span style={{ color: 'var(--error)' }}>*</span>
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
              <div style={{ padding: 12, backgroundColor: 'var(--warning-bg)', borderRadius: 6, fontSize: 13, color: 'var(--warning)' }}>
                {t('agentCreate.warningNoModels')}
              </div>
            ) : (
              <Select
                value={selectedModel}
                onChange={setSelectedModel}
                options={allModels.map((m) => ({
                  value: m.id,
                  label: `${m.label} (${m.providerName})${!m.available ? ` — ${t('settings.labelNoKey')}` : ''}`,
                }))}
                searchable
                style={{ width: '100%' }}
              />
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
                    borderColor: activePreset?.id === p.id ? 'var(--accent)' : 'var(--border-subtle)',
                    borderRadius: 4,
                    backgroundColor: activePreset?.id === p.id ? 'var(--accent-bg)' : 'var(--bg-elevated)',
                    color: activePreset?.id === p.id ? 'var(--accent)' : 'var(--text-primary)',
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
              border: '1px solid var(--border-subtle)',
              borderRadius: 8,
              overflow: 'hidden',
            }}
          >
            {/* 名称 */}
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)' }}>
              <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 4 }}>{t('common.name')}</div>
              <div style={{ fontSize: 15, fontWeight: 600 }}>{name}</div>
            </div>

            {/* 系统提示词 */}
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)' }}>
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
            <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border-subtle)' }}>
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
                backgroundColor: 'var(--error-bg)',
                color: 'var(--error)',
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
          borderTop: '1px solid var(--border-subtle)',
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
              backgroundColor: canNext() ? 'var(--accent)' : 'var(--border-subtle)',
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
              backgroundColor: creating ? 'var(--text-secondary)' : 'var(--success)',
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
