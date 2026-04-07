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
import ProviderModelSelector from '../components/ProviderModelSelector'

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

/** 模板分类 */
type TemplateCategory = 'general' | 'dev' | 'creative' | 'work' | 'team'

interface Template {
  nameKey: string
  descKey: string
  prompt: string
  icon: string
  category: TemplateCategory
  model?: string
  temperature?: number
}

/** 分类顺序 */
const CATEGORY_ORDER: TemplateCategory[] = ['general', 'dev', 'creative', 'work', 'team']

/** 分类 i18n key 映射 */
const CATEGORY_KEYS: Record<TemplateCategory, string> = {
  general: 'agentCreate.catGeneral',
  dev: 'agentCreate.catDev',
  creative: 'agentCreate.catCreative',
  work: 'agentCreate.catWork',
  team: 'agentCreate.catTeam',
}

/** 预置 Agent 模板 */
const TEMPLATES: Template[] = [
  // ── 通用 ──
  { nameKey: 'chatPage.templateGeneral', descKey: 'agentCreate.templateGeneralDesc', prompt: '你是一个有用的AI助手，擅长回答各种问题。', icon: 'AI', category: 'general' },
  { nameKey: 'agentCreate.tplTutorName', descKey: 'agentCreate.tplTutorDesc', icon: 'E', category: 'general', model: 'claude-sonnet-4-6', temperature: 0.7, prompt: '你是一位耐心的学习导师，采用苏格拉底式教学法。\n\n核心原则：\n- 不直接给出答案，而是通过提问引导学生自己思考和发现\n- 用简单易懂的语言和生动的类比解释复杂概念\n- 根据学生的水平动态调整讲解深度\n- 每次讲解后用小问题检验理解程度\n- 鼓励学生提出疑问，营造安全的学习氛围\n\n回答格式：\n1. 先确认学生的理解程度\n2. 用类比或故事引入概念\n3. 逐步深入，层层递进\n4. 总结要点，给出思考题' },
  { nameKey: 'agentCreate.tplCustomerServiceName', descKey: 'agentCreate.tplCustomerServiceDesc', icon: 'CS', category: 'general', model: 'gpt-4o-mini', temperature: 0.3, prompt: '你是一位专业、友善、耐心的客户服务助手。\n\n核心原则：\n- 始终保持礼貌和耐心，即使面对重复或情绪化的问题\n- 先理解客户的核心诉求，再提供解决方案\n- 给出清晰、分步的操作指引\n- 无法解决的问题，说明原因并建议转人工\n- 使用温暖但专业的语气\n\n回答规范：\n1. 先表示理解客户的问题\n2. 提供具体的解决步骤\n3. 询问是否还有其他需要帮助的地方' },

  // ── 开发 ──
  { nameKey: 'agentCreate.tplCodeName', descKey: 'agentCreate.tplCodeDesc', icon: '<>', category: 'dev', model: 'claude-sonnet-4-6', temperature: 0.2, prompt: '你是一位资深全栈编程专家，精通多种编程语言和框架。\n\n核心能力：\n- 代码编写：Python, TypeScript, Rust, Go, Java 等主流语言\n- 代码审查：发现潜在bug、安全漏洞、性能瓶颈\n- 调试诊断：根据错误信息和日志定位问题\n- 架构设计：给出可维护、可扩展的设计建议\n\n回答规范：\n1. 先理解需求和上下文\n2. 给出完整可运行的代码，附带必要注释\n3. 解释关键设计决策\n4. 提醒潜在的边界条件和注意事项\n5. 如有多种方案，说明各自优劣' },
  { nameKey: 'agentCreate.tplDataAnalystName', descKey: 'agentCreate.tplDataAnalystDesc', icon: '#', category: 'dev', model: 'gpt-4o', temperature: 0.3, prompt: '你是一位资深数据分析师，擅长从数据中发现洞察。\n\n核心能力：\n- SQL 查询编写与优化（MySQL, PostgreSQL, BigQuery）\n- Python 数据处理（Pandas, NumPy, Matplotlib, Seaborn）\n- 统计分析：假设检验、回归分析、A/B 测试\n- 数据可视化：图表选择、仪表盘设计\n- 业务分析：指标拆解、归因分析、趋势预测\n\n回答规范：\n1. 先理清分析目标和数据结构\n2. 给出完整的分析思路和代码\n3. 解读结果的业务含义\n4. 指出数据质量问题或分析局限性' },

  // ── 创作 ──
  { nameKey: 'agentCreate.tplTranslatorName', descKey: 'agentCreate.tplTranslatorDesc', icon: 'Aa', category: 'creative', model: 'gpt-4o', temperature: 0.3, prompt: '你是一位精通多语种的专业翻译专家，熟悉中文、英语、日语、韩语。\n\n核心原则：\n- 准确传达原文含义，不遗漏不曲解\n- 保持原文的语气、风格和情感色彩\n- 译文自然流畅，符合目标语言的表达习惯\n- 专业术语翻译准确统一，必要时附注原文\n- 文化差异的内容做适当本地化处理\n\n回答规范：\n1. 直接给出译文\n2. 对有争议的翻译选择附注说明\n3. 如有文化背景需要解释，简要注释' },
  { nameKey: 'agentCreate.tplWriterName', descKey: 'agentCreate.tplWriterDesc', icon: 'W', category: 'creative', model: 'claude-sonnet-4-6', temperature: 0.8, prompt: '你是一位专业的写作助手，擅长各类文体的创作与润色。\n\n核心能力：\n- 文章写作：散文、议论文、叙事文、说明文\n- 文案创作：广告文案、社交媒体文案、品牌故事\n- 内容润色：修改语病、优化表达、提升文采\n- 创意写作：小说、诗歌、剧本、段子\n\n回答规范：\n1. 先确认写作目的、目标读者和风格要求\n2. 提供完整的作品，注重结构和节奏\n3. 可根据要求调整语气（正式/轻松/幽默/严肃）\n4. 润色时保留原文核心意思，标注主要修改' },
  { nameKey: 'agentCreate.templateCreative', descKey: 'agentCreate.templateCreativeDesc', prompt: '你是一位创意顾问。擅长头脑风暴、创意方案设计、营销策划。善于跳出常规思维，提供新颖独特的视角和解决方案。', icon: '*', category: 'creative' },

  // ── 工作 ──
  { nameKey: 'agentCreate.tplProductManagerName', descKey: 'agentCreate.tplProductManagerDesc', icon: 'PM', category: 'work', model: 'gpt-4o', temperature: 0.5, prompt: '你是一位经验丰富的产品经理，擅长从用户需求到产品落地的全流程。\n\n核心能力：\n- 需求分析：用户调研、痛点挖掘、需求优先级排序\n- PRD 撰写：功能描述、用户故事、验收标准\n- 竞品分析：市场定位、差异化、SWOT 分析\n- 用户体验：信息架构、交互流程、可用性评估\n- 项目管理：里程碑规划、风险识别\n\n回答规范：\n1. 从用户视角出发分析问题\n2. 给出结构化的文档或分析\n3. 用数据和案例支撑观点\n4. 平衡用户需求、技术可行性和商业价值' },
  { nameKey: 'agentCreate.tplDailyReportName', descKey: 'agentCreate.tplDailyReportDesc', icon: 'DR', category: 'work', model: 'gpt-4o-mini', temperature: 0.3, prompt: '你是一位专业的工作汇报助手，帮助整理和撰写工作日报、周报、月报。\n\n核心能力：\n- 信息整理：将零散的工作内容结构化\n- 亮点提炼：突出关键成果和数据\n- 问题总结：归纳遇到的困难和解决方案\n- 计划梳理：整理下一步工作计划\n\n回答规范：\n1. 采用简洁清晰的要点式格式\n2. 按「已完成 / 进行中 / 计划中」分类\n3. 用量化数据体现工作成果\n4. 突出重点，避免流水账\n\n请将你的工作内容告诉我，我来帮你整理成规范的报告。' },

  // ── 团队（多 Agent 协作模板） ──
  { nameKey: 'agentCreate.tplTeamCodeReview', descKey: 'agentCreate.tplTeamCodeReviewDesc', icon: 'CR', category: 'team' as TemplateCategory, model: 'gpt-4o', prompt: '你是一个代码审查团队的协调者。你会扮演三个角色协作：\n1. 开发者：编写高质量代码\n2. 审查者：检查 bug、安全、性能\n3. 架构师：从系统设计角度给建议\n\n每次回复先以「开发者」角色实现，再以「审查者」角色审查，最后以「架构师」角色给出改进建议。用 --- 分隔三个角色的输出。' },
  { nameKey: 'agentCreate.tplTeamResearch', descKey: 'agentCreate.tplTeamResearchDesc', icon: 'RS', category: 'team' as TemplateCategory, model: 'gpt-4o', prompt: '你是一个研究团队的协调者。你会按三个阶段工作：\n1. 搜索阶段：使用 web_search 收集资料\n2. 分析阶段：提炼关键观点、构建论证\n3. 撰写阶段：输出结构清晰的研究报告\n\n每次研究任务都经过这三个阶段，确保结论有据可查。' },
  { nameKey: 'agentCreate.tplTeamContent', descKey: 'agentCreate.tplTeamContentDesc', icon: 'CT', category: 'team' as TemplateCategory, model: 'gpt-4o', temperature: 0.8, prompt: '你是一个内容创作团队的协调者。工作流程：\n1. 策划：分析受众、确定主题、设计大纲\n2. 创作：根据大纲写出高质量内容\n3. 编辑：润色、校对、优化表达\n\n输出时标注每个阶段的思考过程和最终成果。' },
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
  const [systemPrompt, setSystemPrompt] = useState(t('chatPage.defaultPrompt'))

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
              id: `${p.id}/${m.id}`,
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
  const applyTemplate = (tpl: Template) => {
    setName(t(tpl.nameKey))
    setSystemPrompt(tpl.prompt)
    if (tpl.model) {
      const match = allModels.find((m) => m.id === tpl.model || m.id.endsWith(`/${tpl.model}`))
      if (match) setSelectedModel(match.id)
    }
    if (tpl.temperature != null) {
      setTemperature(tpl.temperature)
    }
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
      if (config.model) {
        const match = allModels.find((m) => m.id === config.model || m.id.endsWith(`/${config.model}`))
        if (match) setSelectedModel(match.id)
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

          {/* 模板选择（按分类展示） */}
          <div style={{ marginBottom: 20 }}>
            <label style={{ display: 'block', fontSize: 13, color: 'var(--text-secondary)', marginBottom: 8 }}>
              {t('agentCreate.quickTemplates')}
            </label>
            {CATEGORY_ORDER.map((cat) => {
              const catTemplates = TEMPLATES.filter((tpl) => tpl.category === cat)
              if (catTemplates.length === 0) return null
              return (
                <div key={cat} style={{ marginBottom: 16 }}>
                  <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6 }}>
                    {t(CATEGORY_KEYS[cat])}
                  </div>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))', gap: 8 }}>
                    {catTemplates.map((tpl, idx) => {
                      const tplName = t(tpl.nameKey)
                      const isSelected = name === tplName
                      return (
                        <button
                          key={tpl.nameKey || idx}
                          onClick={() => applyTemplate(tpl)}
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
                          {tpl.descKey && <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>{t(tpl.descKey)}</div>}
                        </button>
                      )
                    })}
                  </div>
                </div>
              )
            })}
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
            <ProviderModelSelector
              value={selectedModel}
              onChange={setSelectedModel}
              requireKey={false}
              style={{ width: '100%' }}
            />
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
