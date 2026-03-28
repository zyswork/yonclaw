/**
 * 首次启动引导页 — 深色毛玻璃风格
 *
 * 步骤：欢迎 -> 技能展示 -> AI 配置 -> 完成
 */

import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'

const TOTAL_STEPS = 4

// 主题色
const ACCENT = '#10b981'
const ACCENT_END = '#06b6d4'

export default function SetupPage({ onComplete }: { onComplete: () => void }) {
  const { t } = useI18n()
  const [step, setStep] = useState(0)
  const [setupStatus, setSetupStatus] = useState<Record<string, string>>({})
  const [skills, setSkills] = useState<{ name: string; desc: string }[]>([])
  const [providers, setProviders] = useState<{ name: string; hasKey: boolean }[]>([])
  const [apiKey, setApiKey] = useState('')
  const [apiUrl, setApiUrl] = useState('')

  useEffect(() => {
    runAutoSetup()
  }, [])

  const runAutoSetup = async () => {
    // 环境检查
    setSetupStatus(s => ({ ...s, env: 'running' }))
    try { await invoke('health_check'); setSetupStatus(s => ({ ...s, env: 'done' })) }
    catch { setSetupStatus(s => ({ ...s, env: 'done' })) }

    // Node.js
    setSetupStatus(s => ({ ...s, node: 'running' }))
    try {
      const rt = await invoke<{ installed: boolean }>('check_runtime')
      if (!rt?.installed) await invoke('setup_runtime')
      setSetupStatus(s => ({ ...s, node: 'done' }))
    } catch { setSetupStatus(s => ({ ...s, node: 'skip' })) }

    // 默认 Agent
    try {
      const agents = await invoke<Array<{ id: string; name: string }>>('list_agents')
      if (!agents || agents.length === 0) {
        await invoke('create_agent', {
          name: t('chatPage.templateGeneral'), systemPrompt: '你是一个有用的AI助手，擅长回答各种问题。', model: 'gpt-4o',
        })
      }
      setSetupStatus(s => ({ ...s, agent: 'done' }))
    } catch { setSetupStatus(s => ({ ...s, agent: 'skip' })) }

    // 加载技能
    try {
      const agents = await invoke<Array<{ id: string; name: string }>>('list_agents')
      if (agents?.length) {
        const list = await invoke<Array<{ name: string; description?: string }>>('list_skills', { agentId: agents[0].id })
        setSkills((list || []).map((s) => ({ name: s.name, desc: s.description || '' })))
      }
    } catch { /* ignore */ }

    // 加载 providers
    try {
      const p = await invoke<Array<{ name: string; apiKey?: string; enabled: boolean }>>('get_providers')
      setProviders((p || []).map((x) => ({ name: x.name, hasKey: !!(x.apiKey && x.enabled) })))
    } catch { /* ignore */ }
  }

  const handleSaveProvider = async () => {
    if (!apiKey.trim()) return
    try {
      const p = await invoke<Array<{ name: string; apiKey?: string; enabled: boolean; id?: string }>>('get_providers') || []
      const custom = {
        id: 'custom-' + Date.now(),
        name: t('settingsExtra.customProvider'),
        apiType: 'openai',
        baseUrl: apiUrl.trim() || 'https://api.openai.com/v1',
        apiKey: apiKey.trim(),
        models: [{ id: 'gpt-4o', name: 'GPT-4o' }, { id: 'gpt-4o-mini', name: 'GPT-4o Mini' }],
        enabled: true,
      }
      p.push(custom)
      await invoke('set_setting', { key: 'providers', value: JSON.stringify(p) })
      setProviders(prev => [...prev, { name: custom.name, hasKey: true }])
      setApiKey('')
    } catch (e) { toast.error(t('settingsExtra.saveFailed') + ': ' + e) }
  }

  // 默认技能列表（后端未返回时使用）
  const defaultSkills = [
    { name: 'memory_write', desc: t('skills.builtinMemoryWrite') },
    { name: 'memory_read', desc: t('skills.builtinMemoryRead') },
    { name: 'bash_exec', desc: t('skills.builtinBashExec') },
    { name: 'file_read', desc: t('skills.builtinFileRead') },
    { name: 'web_fetch', desc: t('skills.builtinWebFetch') },
    { name: 'provider_manage', desc: t('skills.builtinProviderManage') },
    { name: 'agent_self_config', desc: t('skills.builtinAgentSelfConfig') },
  ]

  const displaySkills = skills.length > 0 ? skills : defaultSkills

  const pages = [
    // Step 0: 欢迎
    () => (
      <div>
        <h1 style={styles.title}>
          {t('setup.welcomeTitle')}
        </h1>
        <p style={styles.subtitle}>
          {t('setup.welcomeDesc')}
        </p>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {[
            { label: t('setup.stepEnvCheck'), status: setupStatus.env },
            { label: t('setup.stepNodejs'), status: setupStatus.node },
            { label: t('setup.stepInitWorkspace'), status: setupStatus.agent },
          ].map((item, i) => (
            <div key={i} style={styles.checkRow}>
              <span style={{ fontSize: 14 }}>{item.label}</span>
              {item.status === 'done' && (
                <span style={{ width: 8, height: 8, borderRadius: '50%', backgroundColor: ACCENT }} />
              )}
              {item.status === 'running' && (
                <span style={{ fontSize: 12, color: 'rgba(255,255,255,0.4)' }}>...</span>
              )}
              {item.status === 'skip' && (
                <span style={{ fontSize: 12, color: 'rgba(255,255,255,0.3)' }}>{t('common.skip')}</span>
              )}
              {!item.status && (
                <span style={{ width: 8, height: 8, borderRadius: '50%', border: '1px solid rgba(255,255,255,0.15)' }} />
              )}
            </div>
          ))}
        </div>
      </div>
    ),

    // Step 1: 技能展示
    () => (
      <div>
        <h1 style={styles.title}>
          {t('setup.skillsTitle')}
        </h1>
        <p style={styles.subtitle}>
          {t('setup.skillsDesc')}
        </p>

        <div style={styles.glassCard}>
          {displaySkills.map((skill, i, arr) => (
            <div key={i} style={{
              display: 'flex', alignItems: 'center', padding: '12px 16px',
              borderBottom: i < arr.length - 1 ? '1px solid rgba(255,255,255,0.06)' : 'none',
            }}>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 13, fontWeight: 500, color: '#f0f0f5' }}>{skill.name}</div>
                {skill.desc && <div style={{ fontSize: 11, color: 'rgba(255,255,255,0.4)', marginTop: 2 }}>{skill.desc}</div>}
              </div>
              <span style={{ width: 8, height: 8, borderRadius: '50%', backgroundColor: ACCENT }} />
            </div>
          ))}
        </div>
      </div>
    ),

    // Step 2: AI 配置
    () => (
      <div>
        <h1 style={styles.title}>
          {t('setup.aiConfigTitle')}
        </h1>
        <p style={styles.subtitle}>
          {t('setup.aiConfigDesc')}
        </p>

        {providers.some(p => p.hasKey) ? (
          <div style={styles.glassCard}>
            {providers.filter(p => p.hasKey).map((p, i) => (
              <div key={i} style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                padding: '14px 16px', borderBottom: '1px solid rgba(255,255,255,0.06)',
              }}>
                <span style={{ fontSize: 14, color: '#f0f0f5' }}>{p.name}</span>
                <span style={{ width: 8, height: 8, borderRadius: '50%', backgroundColor: ACCENT }} />
              </div>
            ))}
          </div>
        ) : (
          <div style={styles.glassCard}>
            <div style={{ padding: 20 }}>
              <div style={{ marginBottom: 14 }}>
                <label style={styles.inputLabel}>API Key</label>
                <input
                  value={apiKey} onChange={e => setApiKey(e.target.value)}
                  placeholder="sk-..."
                  type="password"
                  style={styles.input}
                />
              </div>
              <div style={{ marginBottom: 16 }}>
                <label style={styles.inputLabel}>Base URL</label>
                <input
                  value={apiUrl} onChange={e => setApiUrl(e.target.value)}
                  placeholder="https://api.openai.com/v1"
                  style={styles.input}
                />
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button onClick={handleSaveProvider} disabled={!apiKey.trim()} style={{
                  ...styles.btnAccent,
                  opacity: apiKey.trim() ? 1 : 0.4,
                }}>
                  {t('setup.btnSaveConfig')}
                </button>
                <button onClick={async () => {
                  if (!apiKey.trim()) return
                  try {
                    const result = await invoke<any>('test_provider_connection', {
                      apiType: 'openai', apiKey: apiKey.trim(),
                      baseUrl: apiUrl.trim() || null,
                    })
                    toast.success(`Connection OK (${result.latency_ms}ms, ${result.models_available} models)`)
                  } catch (e) { toast.error('Connection failed: ' + String(e)) }
                }} disabled={!apiKey.trim()} style={{
                  ...styles.btnGhost,
                  opacity: apiKey.trim() ? 1 : 0.4,
                }}>
                  {t('settings.testBtn') || 'Test'}
                </button>
              </div>
            </div>
          </div>
        )}

        <p style={{ color: 'rgba(255,255,255,0.35)', fontSize: 12, marginTop: 16 }}>
          {t('setup.hintLater')}
        </p>
      </div>
    ),

    // Step 3: 完成
    () => (
      <div>
        <h1 style={styles.title}>
          {t('setup.completionTitle')}
        </h1>
        <p style={styles.subtitle}>
          {t('setup.completionDesc')}
        </p>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
          {[
            { label: t('setup.tipDirectChat'), desc: t('setup.tipDirectChatDesc') },
            { label: t('setup.tipSlashCommands'), desc: t('setup.tipSlashCommandsDesc') },
            { label: t('setup.tipImageInput'), desc: t('setup.tipImageInputDesc') },
            { label: t('setup.tipMemory'), desc: t('setup.tipMemoryDesc') },
          ].map((item, i) => (
            <div key={i} style={styles.checkRow}>
              <div>
                <div style={{ fontSize: 14, fontWeight: 600, color: '#f0f0f5' }}>{item.label}</div>
                <div style={{ fontSize: 12, color: 'rgba(255,255,255,0.4)', marginTop: 2 }}>{item.desc}</div>
              </div>
            </div>
          ))}
        </div>
      </div>
    ),
  ]

  return (
    <div style={styles.backdrop}>
      <div style={styles.container}>
        {/* 品牌名 */}
        <div style={{ textAlign: 'center', marginBottom: 32 }}>
          <h2 style={styles.brand}>衔烛</h2>
          <div style={{ fontSize: 12, color: 'rgba(255,255,255,0.35)', letterSpacing: 1 }}>XianZhu AI</div>
        </div>

        {/* 步骤指示器：水平进度条 + 编号圆圈 */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 0, marginBottom: 36 }}>
          {Array.from({ length: TOTAL_STEPS }, (_, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center' }}>
              {/* 步骤圆圈 */}
              <div style={{
                width: 28, height: 28, borderRadius: '50%',
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                fontSize: 12, fontWeight: 600,
                backgroundColor: i <= step ? ACCENT : 'rgba(255,255,255,0.08)',
                color: i <= step ? '#fff' : 'rgba(255,255,255,0.3)',
                transition: 'all 0.3s ease',
                border: i === step ? `2px solid ${ACCENT}` : '2px solid transparent',
                boxShadow: i === step ? `0 0 12px ${ACCENT}40` : 'none',
              }}>
                {i + 1}
              </div>
              {/* 连接线 */}
              {i < TOTAL_STEPS - 1 && (
                <div style={{
                  width: 48, height: 2, borderRadius: 1,
                  backgroundColor: i < step ? ACCENT : 'rgba(255,255,255,0.08)',
                  transition: 'background-color 0.3s ease',
                }} />
              )}
            </div>
          ))}
        </div>

        {/* 内容卡片 */}
        <div style={styles.contentCard}>
          {pages[step]()}
        </div>

        {/* 底部导航 */}
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 24 }}>
          <button
            onClick={onComplete}
            style={styles.btnSkip}
          >
            {t('common.skip')}
          </button>
          <div style={{ display: 'flex', gap: 10 }}>
            {step > 0 && (
              <button
                onClick={() => setStep(step - 1)}
                style={styles.btnGhost}
              >
                {t('common.prev')}
              </button>
            )}
            <button
              onClick={() => step < TOTAL_STEPS - 1 ? setStep(step + 1) : onComplete()}
              style={styles.btnAccent}
            >
              {step < TOTAL_STEPS - 1 ? t('common.next') : t('common.start')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

// 内联样式
const styles: Record<string, React.CSSProperties> = {
  backdrop: {
    minHeight: '100vh',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'linear-gradient(160deg, #0a0a0f 0%, #0f172a 50%, #0a0a0f 100%)',
    fontFamily: '-apple-system, BlinkMacSystemFont, system-ui, sans-serif',
    padding: 20,
  },
  container: {
    width: '100%',
    maxWidth: 500,
  },
  brand: {
    margin: 0,
    fontSize: 28,
    fontWeight: 700,
    background: `linear-gradient(135deg, ${ACCENT}, ${ACCENT_END})`,
    WebkitBackgroundClip: 'text',
    WebkitTextFillColor: 'transparent',
    letterSpacing: 4,
  },
  contentCard: {
    padding: '28px 24px',
    borderRadius: 16,
    background: 'rgba(18, 18, 28, 0.75)',
    backdropFilter: 'blur(24px)',
    WebkitBackdropFilter: 'blur(24px)',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    boxShadow: '0 16px 48px rgba(0, 0, 0, 0.4)',
  },
  title: {
    fontSize: 22,
    fontWeight: 700,
    margin: '0 0 8px',
    color: '#f0f0f5',
  },
  subtitle: {
    color: 'rgba(255,255,255,0.45)',
    fontSize: 14,
    lineHeight: 1.7,
    margin: '0 0 24px',
  },
  glassCard: {
    borderRadius: 12,
    background: 'rgba(255, 255, 255, 0.03)',
    border: '1px solid rgba(255, 255, 255, 0.08)',
    overflow: 'hidden',
  },
  checkRow: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '13px 16px',
    borderRadius: 10,
    background: 'rgba(255, 255, 255, 0.03)',
    border: '1px solid rgba(255, 255, 255, 0.06)',
  },
  inputLabel: {
    fontSize: 12,
    color: 'rgba(255,255,255,0.45)',
    display: 'block',
    marginBottom: 6,
  },
  input: {
    width: '100%',
    padding: '10px 14px',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    fontSize: 14,
    boxSizing: 'border-box' as const,
    background: 'rgba(255,255,255,0.04)',
    color: '#f0f0f5',
    outline: 'none',
  },
  btnAccent: {
    padding: '10px 28px',
    background: `linear-gradient(135deg, ${ACCENT}, ${ACCENT_END})`,
    color: '#fff',
    border: 'none',
    borderRadius: 8,
    fontSize: 14,
    fontWeight: 500,
    cursor: 'pointer',
    transition: 'opacity 0.2s',
  },
  btnGhost: {
    padding: '10px 20px',
    backgroundColor: 'transparent',
    color: 'rgba(255,255,255,0.6)',
    border: '1px solid rgba(255,255,255,0.1)',
    borderRadius: 8,
    fontSize: 13,
    cursor: 'pointer',
  },
  btnSkip: {
    background: 'none',
    border: 'none',
    color: 'rgba(255,255,255,0.3)',
    fontSize: 13,
    cursor: 'pointer',
    padding: '8px 0',
  },
}
