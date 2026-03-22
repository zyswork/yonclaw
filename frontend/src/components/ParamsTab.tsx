/**
 * 参数 Tab
 *
 * 功能：
 * - 温度预设（精确/均衡/创造）
 * - 模型选择（仅显示有 API Key 的模型）
 * - 高级参数（温度滑块、最大 Token）
 */

import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface ParamsTabProps {
  agentId: string
}

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

interface ModelOption {
  id: string
  label: string
  provider: string
  providerName: string
}

/** 温度预设 */
const TEMP_PRESETS = [
  { id: 'precise', value: 0.2 },
  { id: 'balanced', value: 0.7 },
  { id: 'creative', value: 1.2 },
]

export default function ParamsTab({ agentId }: ParamsTabProps) {
  const { t } = useI18n()
  const presetLabels: Record<string, string> = {
    precise: t('paramsTab.precise'),
    balanced: t('paramsTab.balanced'),
    creative: t('paramsTab.creative'),
  }
  const [model, setModel] = useState('')
  const [temperature, setTemperature] = useState(0.7)
  const [maxTokens, setMaxTokens] = useState(4096)
  const [models, setModels] = useState<ModelOption[]>([])
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [saving, setSaving] = useState(false)
  const [status, setStatus] = useState('')
  const [loading, setLoading] = useState(true)

  /** 加载可用模型列表 */
  const loadModels = useCallback(async () => {
    try {
      const providers = await invoke<Provider[]>('get_providers')
      const opts: ModelOption[] = []
      for (const p of providers || []) {
        if (!p.enabled) continue
        const hasKey = !!p.apiKeyMasked && p.apiKeyMasked !== ''
        const isLocal = p.id === 'ollama' || p.baseUrl?.includes('localhost')
        if (!hasKey && !isLocal) continue
        for (const m of p.models || []) {
          opts.push({ id: m.id, label: m.name, provider: p.id, providerName: p.name })
        }
      }
      setModels(opts)
    } catch (e) {
      console.error('加载模型列表失败:', e)
    }
  }, [])

  /** 加载当前 Agent 参数 */
  const loadAgentParams = useCallback(async () => {
    setLoading(true)
    try {
      // 通过 list_agents 获取当前 agent 信息
      const agents = await invoke<Array<{
        id: string; model: string; temperature?: number; maxTokens?: number
      }>>('list_agents')
      const agent = agents?.find(a => a.id === agentId)
      if (agent) {
        setModel(agent.model || '')
        setTemperature(agent.temperature ?? 0.7)
        setMaxTokens(agent.maxTokens ?? 4096)
      }
    } catch (e) {
      console.error('加载 Agent 参数失败:', e)
    } finally {
      setLoading(false)
    }
  }, [agentId])

  useEffect(() => {
    loadModels()
    loadAgentParams()
  }, [loadModels, loadAgentParams])

  /** 保存参数 */
  const handleSave = async () => {
    setSaving(true)
    setStatus('')
    try {
      await invoke('update_agent', { agentId, model, temperature, maxTokens })
      setStatus(t('paramsTab.saved'))
      setTimeout(() => setStatus(''), 2000)
    } catch (e) {
      setStatus(t('paramsTab.saveFailed') + ': ' + String(e))
    } finally {
      setSaving(false)
    }
  }

  /** 当前温度匹配哪个预设 */
  const activePreset = TEMP_PRESETS.find(p => Math.abs(p.value - temperature) < 0.05)

  if (loading) {
    return <div style={{ padding: '20px', textAlign: 'center', color: '#999', fontSize: '13px' }}>{t('common.loading')}</div>
  }

  return (
    <div style={{ padding: '8px 0' }}>
      {/* 温度预设 */}
      <div style={{ marginBottom: '14px' }}>
        <div style={{ fontSize: '12px', color: '#666', marginBottom: '6px' }}>{t('paramsTab.tempPreset')}</div>
        <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
          {TEMP_PRESETS.map(p => (
            <button
              key={p.id}
              onClick={() => setTemperature(p.value)}
              style={{
                padding: '4px 12px', fontSize: '12px', border: '1px solid',
                borderColor: activePreset?.id === p.id ? 'var(--accent)' : '#ddd',
                borderRadius: '4px', cursor: 'pointer',
                backgroundColor: activePreset?.id === p.id ? 'var(--accent)' : 'white',
                color: activePreset?.id === p.id ? 'white' : '#333',
              }}
            >
              {presetLabels[p.id] || p.id}
            </button>
          ))}
          {!activePreset && (
            <span style={{ fontSize: '11px', color: '#999', marginLeft: '4px' }}>{t('paramsTab.custom')}</span>
          )}
        </div>
      </div>

      {/* 模型选择 */}
      <div style={{ marginBottom: '14px' }}>
        <div style={{ fontSize: '12px', color: '#666', marginBottom: '6px' }}>{t('paramsTab.selectModel')}</div>
        <select
          value={model}
          onChange={e => setModel(e.target.value)}
          style={{
            width: '100%', padding: '6px 8px', border: '1px solid #ddd',
            borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box',
          }}
        >
          {model && !models.some(m => m.id === model) && (
            <option value={model}>{model} {t('paramsTab.current')}</option>
          )}
          {models.map(m => (
            <option key={`${m.provider}-${m.id}`} value={m.id}>
              {m.label} ({m.providerName})
            </option>
          ))}
        </select>
      </div>

      {/* 高级参数折叠区 */}
      <div style={{ marginBottom: '14px' }}>
        <button
          onClick={() => setShowAdvanced(!showAdvanced)}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            fontSize: '12px', color: '#666', padding: '4px 0',
            display: 'flex', alignItems: 'center', gap: '4px',
          }}
        >
          <span style={{
            display: 'inline-block', transition: 'transform 0.2s',
            transform: showAdvanced ? 'rotate(90deg)' : 'rotate(0deg)',
          }}>
            ▶
          </span>
          {t('paramsTab.advancedParams')}
        </button>

        {showAdvanced && (
          <div style={{ marginTop: '8px', padding: '8px', backgroundColor: '#fafafa', borderRadius: '4px' }}>
            {/* 温度滑块 */}
            <div style={{ marginBottom: '12px' }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '4px' }}>
                <label style={{ fontSize: '12px', color: '#666' }}>Temperature</label>
                <span style={{ fontSize: '12px', color: '#333', fontWeight: 500 }}>{temperature.toFixed(1)}</span>
              </div>
              <input
                type="range"
                min="0" max="2" step="0.1"
                value={temperature}
                onChange={e => setTemperature(parseFloat(e.target.value))}
                style={{ width: '100%' }}
              />
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '10px', color: '#999' }}>
                <span>{t('paramsTab.labelPrecise')}</span>
                <span>{t('paramsTab.labelRandom')}</span>
              </div>
            </div>

            {/* Max Tokens */}
            <div>
              <label style={{ fontSize: '12px', color: '#666', display: 'block', marginBottom: '4px' }}>Max Tokens</label>
              <input
                type="number"
                value={maxTokens}
                onChange={e => setMaxTokens(parseInt(e.target.value) || 0)}
                min={1}
                max={128000}
                style={{
                  width: '100%', padding: '6px 8px', border: '1px solid #ddd',
                  borderRadius: '4px', fontSize: '13px', boxSizing: 'border-box',
                }}
              />
            </div>
          </div>
        )}
      </div>

      {/* 保存按钮 */}
      <button
        onClick={handleSave}
        disabled={saving}
        style={{
          width: '100%', padding: '8px', backgroundColor: 'var(--accent)',
          color: 'white', border: 'none', borderRadius: '4px', fontSize: '13px',
          cursor: saving ? 'not-allowed' : 'pointer', opacity: saving ? 0.6 : 1,
        }}
      >
        {saving ? t('common.saving') : t('paramsTab.saveParams')}
      </button>

      {status && (
        <div style={{
          fontSize: '12px', marginTop: '6px', textAlign: 'center',
          color: status.startsWith(t('paramsTab.saveFailed')) ? 'var(--error)' : 'var(--success)',
        }}>
          {status}
        </div>
      )}
    </div>
  )
}
