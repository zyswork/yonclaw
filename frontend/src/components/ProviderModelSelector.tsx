/**
 * 供应商 + 模型二级选择器
 *
 * 先选供应商，再选该供应商下的模型。
 * value / onChange 使用 "providerId/modelId" 格式。
 */

import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
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

interface Props {
  /** 当前值，格式 "providerId/modelId" 或旧格式纯 "modelId" */
  value: string
  onChange: (v: string) => void
  /** 是否只显示有 key 的供应商（默认 true） */
  requireKey?: boolean
  style?: React.CSSProperties
}

function ChevronDown({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="6 9 12 15 18 9" />
    </svg>
  )
}

function ProviderIcon({ name }: { name: string }) {
  // 首字母缩写徽章
  const initials = name.replace(/[()]/g, '').trim().split(/\s+/).map(w => w[0]).join('').slice(0, 2).toUpperCase()
  const colors: Record<string, string> = {
    'OA': '#10a37f', 'AN': '#d4a574', 'DS': '#4d6ef5', 'ZH': '#1e90ff',
    'MO': '#7c3aed', 'QW': '#ff6b35', 'MI': '#06b6d4', 'BA': '#f59e0b',
    'OL': '#22c55e',
  }
  const bg = colors[initials] || 'var(--accent)'
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
      width: 20, height: 20, borderRadius: 5, fontSize: 9, fontWeight: 700,
      backgroundColor: bg, color: '#fff', flexShrink: 0, letterSpacing: 0.3,
    }}>
      {initials}
    </span>
  )
}

export default function ProviderModelSelector({ value, onChange, requireKey = true, style }: Props) {
  const { t } = useI18n()
  const [providers, setProviders] = useState<Provider[]>([])
  const [selectedProvider, setSelectedProvider] = useState<string>('')
  const [providerOpen, setProviderOpen] = useState(false)
  const [modelOpen, setModelOpen] = useState(false)

  // 解析当前 value
  const slashIdx = value.indexOf('/')
  const currentProviderId = slashIdx >= 0 ? value.slice(0, slashIdx) : ''
  const currentModelId = slashIdx >= 0 ? value.slice(slashIdx + 1) : value

  useEffect(() => {
    ;(async () => {
      try {
        const raw = await invoke<Provider[]>('get_providers')
        const filtered = (raw || []).filter(p => {
          if (!p.enabled) return false
          if (!requireKey) return true
          const hasKey = !!p.apiKeyMasked && p.apiKeyMasked !== ''
          const isLocal = p.id === 'ollama' || p.baseUrl?.includes('localhost')
          return hasKey || isLocal
        })
        setProviders(filtered)
        // 初始化选中的供应商
        if (currentProviderId && filtered.some(p => p.id === currentProviderId)) {
          setSelectedProvider(currentProviderId)
        } else if (filtered.length > 0) {
          setSelectedProvider(filtered[0].id)
        }
      } catch (e) { console.error(e) }
    })()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // value 外部变化时同步供应商
  useEffect(() => {
    if (currentProviderId && providers.some(p => p.id === currentProviderId)) {
      setSelectedProvider(currentProviderId)
    }
  }, [currentProviderId, providers])

  const activeProvider = providers.find(p => p.id === selectedProvider)
  const modelsForProvider = activeProvider?.models || []

  // 当前显示标签
  const currentModel = modelsForProvider.find(m => m.id === currentModelId)
  const displayLabel = currentModel
    ? currentModel.name
    : currentModelId || t('common.pleaseSelect')

  const handleSelectProvider = (pid: string) => {
    setSelectedProvider(pid)
    setProviderOpen(false)
    // 切换供应商时，自动选第一个模型
    const p = providers.find(x => x.id === pid)
    if (p && p.models.length > 0) {
      onChange(`${pid}/${p.models[0].id}`)
    }
  }

  const handleSelectModel = (mid: string) => {
    if (mid === '') {
      // 清除选择
      onChange('')
    } else {
      onChange(`${selectedProvider}/${mid}`)
    }
    setModelOpen(false)
  }

  const selectBase: React.CSSProperties = {
    position: 'relative', width: '100%',
  }

  const triggerBase: React.CSSProperties = {
    width: '100%', padding: '8px 12px',
    border: '1px solid var(--border-subtle)', borderRadius: 8,
    backgroundColor: 'var(--bg-elevated)', color: 'var(--text-primary)',
    cursor: 'pointer', fontSize: 13,
    display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8,
    boxSizing: 'border-box', transition: 'border-color 0.15s',
    outline: 'none',
  }

  const dropdownBase: React.CSSProperties = {
    position: 'absolute', left: 0, right: 0, top: 'calc(100% + 4px)',
    backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
    borderRadius: 8, boxShadow: '0 4px 16px rgba(0,0,0,0.15)',
    zIndex: 200, maxHeight: 220, overflowY: 'auto',
    padding: '4px',
  }

  const itemBase: React.CSSProperties = {
    padding: '8px 10px', borderRadius: 6, cursor: 'pointer', fontSize: 13,
    display: 'flex', alignItems: 'center', gap: 8,
    transition: 'background-color 0.1s',
  }

  return (
    <div style={style}>
      {/* 供应商选择 */}
      <div style={{ marginBottom: 8 }}>
        <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 4, fontWeight: 500, letterSpacing: 0.3 }}>
          {t('common.provider')}
        </div>
        <div style={selectBase}>
          <button
            type="button"
            style={{ ...triggerBase, borderColor: providerOpen ? 'var(--accent)' : 'var(--border-subtle)' }}
            onClick={() => { setProviderOpen(o => !o); setModelOpen(false) }}
          >
            <span style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
              {activeProvider && <ProviderIcon name={activeProvider.name} />}
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {activeProvider?.name || t('common.pleaseSelect')}
              </span>
            </span>
            <span style={{ color: 'var(--text-muted)', flexShrink: 0, transform: providerOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
              <ChevronDown />
            </span>
          </button>
          {providerOpen && (
            <div style={dropdownBase}>
              {providers.map(p => (
                <div
                  key={p.id}
                  onClick={() => handleSelectProvider(p.id)}
                  style={{
                    ...itemBase,
                    backgroundColor: selectedProvider === p.id ? 'var(--accent-bg)' : 'transparent',
                    color: selectedProvider === p.id ? 'var(--accent)' : 'var(--text-primary)',
                    fontWeight: selectedProvider === p.id ? 600 : 400,
                  }}
                  onMouseEnter={e => { if (selectedProvider !== p.id) e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                  onMouseLeave={e => { if (selectedProvider !== p.id) e.currentTarget.style.backgroundColor = 'transparent' }}
                >
                  <ProviderIcon name={p.name} />
                  <span>{p.name}</span>
                  <span style={{ marginLeft: 'auto', fontSize: 11, color: 'var(--text-muted)' }}>
                    {p.models.length} 个模型
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* 模型选择 */}
      <div>
        <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 4, fontWeight: 500, letterSpacing: 0.3 }}>
          {t('common.model')}
        </div>
        <div style={selectBase}>
          <button
            type="button"
            style={{ ...triggerBase, borderColor: modelOpen ? 'var(--accent)' : 'var(--border-subtle)' }}
            onClick={() => { setModelOpen(o => !o); setProviderOpen(false) }}
            disabled={false}
          >
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', color: currentModel ? 'var(--text-primary)' : 'var(--text-muted)' }}>
              {displayLabel}
            </span>
            <span style={{ color: 'var(--text-muted)', flexShrink: 0, transform: modelOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
              <ChevronDown />
            </span>
          </button>
          {modelOpen && (
            <div style={dropdownBase}>
              {/* 清除选项 */}
              <div
                onClick={() => handleSelectModel('')}
                style={{
                  ...itemBase,
                  backgroundColor: !currentModelId ? 'var(--accent-bg)' : 'transparent',
                  color: !currentModelId ? 'var(--accent)' : 'var(--text-muted)',
                  fontStyle: 'italic',
                  borderBottom: '1px solid var(--border-subtle)',
                  marginBottom: 4,
                }}
                onMouseEnter={e => { if (currentModelId) e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                onMouseLeave={e => { if (currentModelId) e.currentTarget.style.backgroundColor = 'transparent' }}
              >
                <span style={{ width: 12, flexShrink: 0 }} />
                <span>-- {t('common.notConfigured')} --</span>
              </div>
              {modelsForProvider.map(m => {
                const isSelected = m.id === currentModelId && selectedProvider === currentProviderId
                return (
                  <div
                    key={m.id}
                    onClick={() => handleSelectModel(m.id)}
                    style={{
                      ...itemBase,
                      backgroundColor: isSelected ? 'var(--accent-bg)' : 'transparent',
                      color: isSelected ? 'var(--accent)' : 'var(--text-primary)',
                      fontWeight: isSelected ? 600 : 400,
                    }}
                    onMouseEnter={e => { if (!isSelected) e.currentTarget.style.backgroundColor = 'var(--bg-glass)' }}
                    onMouseLeave={e => { if (!isSelected) e.currentTarget.style.backgroundColor = 'transparent' }}
                  >
                    {isSelected && (
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none"
                        stroke="var(--accent)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"
                        style={{ flexShrink: 0 }}>
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    )}
                    {!isSelected && <span style={{ width: 12, flexShrink: 0 }} />}
                    <span>{m.name}</span>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </div>

      {/* 点击外部关闭 */}
      {(providerOpen || modelOpen) && (
        <div
          style={{ position: 'fixed', inset: 0, zIndex: 199 }}
          onClick={() => { setProviderOpen(false); setModelOpen(false) }}
        />
      )}
    </div>
  )
}
