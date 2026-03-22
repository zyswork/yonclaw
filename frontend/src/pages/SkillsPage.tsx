/**
 * 技能广场 — Marketplace + 多 Agent 安装管理
 *
 * - 从 ~/.yonclaw/marketplace/ 加载全局可用技能
 * - 从 agent skills/ 加载已安装技能
 * - 支持给指定 Agent 安装/卸载技能
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'

interface MarketplaceSkill {
  name: string
  description: string
  tools_count: number
  trigger_keywords: string[]
}

interface InstalledSkill {
  name: string
  description: string
  enabled: boolean
  source: string
}

// 技能元数据（icon + 分类 key）
const SKILL_META: Record<string, { icon: string; category: string }> = {
  // productivity
  'mail-ops':         { icon: '\u{1F4E8}', category: 'productivity' },
  'oa-schedule':      { icon: '\u{1F4C5}', category: 'productivity' },
  'oa-task':          { icon: '\u2705',     category: 'productivity' },
  'oa-meeting':       { icon: '\u{1F4C5}', category: 'productivity' },
  'oa-common':        { icon: '\u{1F527}', category: 'system' },
  'summarize':        { icon: '\u{1F4DD}', category: 'productivity' },
  'weather':          { icon: '\u26C5',     category: 'productivity' },
  'apple-notes':      { icon: '\u{1F4D3}', category: 'productivity' },
  'apple-reminders':  { icon: '\u{1F514}', category: 'productivity' },
  // development
  'github':           { icon: '\u{1F4BB}', category: 'development' },
  'coding-agent':     { icon: '\u{1F916}', category: 'development' },
  'skill-creator':    { icon: '\u2728',     category: 'development' },
  'spec-generator':   { icon: '\u{1F4CB}', category: 'development' },
  'session-logs':     { icon: '\u{1F4DC}', category: 'development' },
  'tmux':             { icon: '\u{1F5A5}\uFE0F', category: 'development' },
  // media
  'nano-banana-pro':  { icon: '\u{1F3A8}', category: 'media' },
  'nano-pdf':         { icon: '\u{1F4C4}', category: 'media' },
  'peekaboo':         { icon: '\u{1F441}\uFE0F', category: 'media' },
  // platform
  'clawhub':          { icon: '\u{1F30D}', category: 'platform' },
}

// 内置工具（始终可用，不需安装）
const BUILTIN_TOOLS = [
  { name: 'memory_write', descKey: 'skills.builtinMemoryWrite', icon: '\u{1F9E0}' },
  { name: 'memory_read', descKey: 'skills.builtinMemoryRead', icon: '\u{1F50D}' },
  { name: 'bash_exec', descKey: 'skills.builtinBashExec', icon: '\u{1F4BB}' },
  { name: 'file_read', descKey: 'skills.builtinFileRead', icon: '\u{1F4C4}' },
  { name: 'file_write', descKey: 'skills.builtinFileWrite', icon: '\u{1F4DD}' },
  { name: 'file_edit', descKey: 'skills.builtinFileEdit', icon: '\u270F\uFE0F' },
  { name: 'file_list', descKey: 'skills.builtinFileList', icon: '\u{1F4C1}' },
  { name: 'code_search', descKey: 'skills.builtinCodeSearch', icon: '\u{1F50E}' },
  { name: 'web_fetch', descKey: 'skills.builtinWebFetch', icon: '\u{1F310}' },
  { name: 'calculator', descKey: 'skills.builtinCalculator', icon: '\u{1F522}' },
  { name: 'date_time', descKey: 'skills.builtinDateTime', icon: '\u{1F552}' },
  { name: 'diff_edit', descKey: 'skills.builtinDiffEdit', icon: '\u{1F4CB}' },
  { name: 'settings_manage', descKey: 'skills.builtinSettingsManage', icon: '\u{1F527}' },
  { name: 'provider_manage', descKey: 'skills.builtinProviderManage', icon: '\u2699\uFE0F' },
  { name: 'agent_self_config', descKey: 'skills.builtinAgentSelfConfig', icon: '\u{1F916}' },
]

const CATEGORY_KEYS = ['all', 'installed', 'available', 'online', 'productivity', 'development', 'media', 'platform', 'builtin']

const CATEGORY_I18N: Record<string, string> = {
  all: 'skills.tabAll',
  installed: 'skills.tabInstalled',
  available: 'skills.tabAvailable',
  online: 'skills.tabOnlineMarket',
  productivity: 'skills.categoryProductivity',
  development: 'skills.categoryDevelopment',
  media: 'skills.categoryMedia',
  platform: 'skills.categoryPlatform',
  system: 'skills.categorySystem',
  builtin: 'skills.categoryBuiltin',
  other: 'skills.categoryOther',
}

export default function SkillsPage() {
  const { t } = useI18n()
  const [marketplace, setMarketplace] = useState<MarketplaceSkill[]>([])
  const [installed, setInstalled] = useState<InstalledSkill[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [activeTab, setActiveTab] = useState('all')
  const [loading, setLoading] = useState(true)
  const [operating, setOperating] = useState('')
  const [onlineSkills, setOnlineSkills] = useState<any[]>([])
  const [onlineSearch, setOnlineSearch] = useState('')
  const [onlineLoading, setOnlineLoading] = useState(false)
  const [downloading, setDownloading] = useState('')
  const [publishing, setPublishing] = useState('')

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { if (selectedAgent) loadAll() }, [selectedAgent])
  useEffect(() => { if (activeTab === 'online') loadOnlineSkills() }, [activeTab])

  const loadAgents = async () => {
    try {
      const list = (await invoke('list_agents')) as any[]
      setAgents(list.map((a: any) => ({ id: a.id, name: a.name })))
      if (list.length > 0) setSelectedAgent(list[0].id)
    } catch { /* ignore */ }
    setLoading(false)
  }

  const loadAll = async () => {
    try {
      const [mp, inst] = await Promise.all([
        invoke<MarketplaceSkill[]>('list_marketplace_skills'),
        invoke<InstalledSkill[]>('list_skills', { agentId: selectedAgent }),
      ])
      setMarketplace(mp)
      setInstalled(inst)
    } catch { /* ignore */ }
  }

  const loadOnlineSkills = async (q?: string) => {
    setOnlineLoading(true)
    try {
      const path = q
        ? `/api/v1/skill-hub/search?q=${encodeURIComponent(q)}`
        : '/api/v1/skill-hub/search'
      const resp = await invoke<any>('cloud_api_proxy', { method: 'GET', path, body: null })
      const data = resp
      setOnlineSkills(data.skills || [])
    } catch (e) {
      console.error('加载在线技能失败:', e)
      // fallback：直接 fetch（如果 WebView 允许）
      try {
        const url = q
          ? `https://zys-openclaw.com/api/v1/skill-hub/search?q=${encodeURIComponent(q)}`
          : 'https://zys-openclaw.com/api/v1/skill-hub/search'
        const resp = await fetch(url)
        const data = await resp.json()
        setOnlineSkills(data.skills || [])
      } catch { setOnlineSkills([]) }
    }
    setOnlineLoading(false)
  }

  const handleDownloadFromHub = async (slug: string) => {
    setDownloading(slug)
    try {
      const msg = await invoke<string>('download_skill_from_hub', { slug })
      await loadAll() // 刷新本地 marketplace 列表
      loadOnlineSkills(onlineSearch) // 刷新在线列表（更新"已有"状态）
      alert(msg)
    } catch (e) { alert(t('skills.downloadFailed') + ': ' + e) }
    setDownloading('')
  }

  const handlePublishToHub = async (skillName: string) => {
    const author = prompt(t('skills.promptAuthor'), '') || ''
    setPublishing(skillName)
    try {
      const msg = await invoke<string>('publish_skill_to_hub', { skillName, author })
      alert(msg)
      if (activeTab === 'online') loadOnlineSkills(onlineSearch)
    } catch (e) { alert(t('skills.publishFailed') + ': ' + e) }
    setPublishing('')
  }

  const installedNames = new Set(installed.map(s => s.name))

  const handleInstall = async (skillName: string) => {
    setOperating(skillName)
    try {
      await invoke('install_skill_to_agent', { agentId: selectedAgent, skillName })
      await loadAll()
    } catch (e) { alert(t('skills.installFailed') + ': ' + e) }
    setOperating('')
  }

  const handleUninstall = async (skillName: string) => {
    if (!confirm(`${t('skills.btnUninstall')} ${skillName}?`)) return
    setOperating(skillName)
    try {
      await invoke('uninstall_skill_from_agent', { agentId: selectedAgent, skillName })
      await loadAll()
    } catch (e) { alert(t('skills.uninstallFailed') + ': ' + e) }
    setOperating('')
  }

  // 构建统一列表
  type DisplaySkill = {
    name: string; desc: string; icon: string; category: string
    installed: boolean; isBuiltin: boolean; tools_count: number
  }

  const allSkills: DisplaySkill[] = [
    // 技能市场的技能
    ...marketplace.map(s => {
      const meta = SKILL_META[s.name] || { icon: '\u{1F9E9}', category: 'other' }
      return {
        name: s.name,
        desc: s.description || '',
        icon: meta.icon,
        category: meta.category,
        installed: installedNames.has(s.name),
        isBuiltin: false,
        tools_count: s.tools_count,
      }
    }),
    // 已安装但不在 marketplace 的技能
    ...installed
      .filter(s => !marketplace.some(m => m.name === s.name) && !BUILTIN_TOOLS.some(b => b.name === s.name))
      .map(s => {
        const meta = SKILL_META[s.name] || { icon: '\u{1F9E9}', category: 'other' }
        return {
          name: s.name, desc: s.description || '', icon: meta.icon,
          category: meta.category, installed: true, isBuiltin: false, tools_count: 0,
        }
      }),
    // 内置工具
    ...BUILTIN_TOOLS.map(b => ({
      name: b.name, desc: t(b.descKey), icon: b.icon, category: 'builtin',
      installed: true, isBuiltin: true, tools_count: 0,
    })),
  ]

  const filtered = activeTab === 'all' ? allSkills.filter(s => !s.isBuiltin)
    : activeTab === 'installed' ? allSkills.filter(s => s.installed && !s.isBuiltin)
    : activeTab === 'available' ? allSkills.filter(s => !s.installed && !s.isBuiltin)
    : activeTab === 'builtin' ? allSkills.filter(s => s.isBuiltin)
    : allSkills.filter(s => s.category === activeTab)

  if (loading) return <div style={{ padding: 24, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  const installedCount = allSkills.filter(s => s.installed && !s.isBuiltin).length
  const availableCount = allSkills.filter(s => !s.installed && !s.isBuiltin).length

  return (
    <div style={{ padding: '24px 32px', maxWidth: 900 }}>
      {/* 标题栏 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 20 }}>
        <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>{t('skills.title')}</h1>
        <span style={{
          fontSize: 12, padding: '2px 8px', borderRadius: 10,
          backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)',
        }}>
          {t('skills.countSkills', { count: marketplace.length })}
        </span>
        <span style={{ flex: 1 }} />
        {/* Agent 选择器 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>{t('skills.installTo')}</span>
          <select
            value={selectedAgent}
            onChange={(e) => setSelectedAgent(e.target.value)}
            style={{ padding: '6px 12px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
          >
            {agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
        </div>
      </div>

      {/* 分类 Tab */}
      <div style={{ display: 'flex', gap: 4, marginBottom: 20, flexWrap: 'wrap' }}>
        {CATEGORY_KEYS.map(cat => {
          const count = cat === 'installed' ? installedCount : cat === 'available' ? availableCount : undefined
          return (
            <button
              key={cat}
              onClick={() => setActiveTab(cat)}
              style={{
                padding: '6px 14px', borderRadius: 16, fontSize: 13, cursor: 'pointer',
                backgroundColor: activeTab === cat ? 'var(--accent)' : 'var(--bg-glass)',
                color: activeTab === cat ? '#fff' : 'var(--text-secondary)',
                border: 'none', fontWeight: activeTab === cat ? 600 : 400,
                transition: 'all 0.15s ease',
              }}
            >
              {t(CATEGORY_I18N[cat] || cat)}{count !== undefined ? ` ${count}` : ''}
            </button>
          )
        })}
      </div>

      {/* 技能列表 */}
      <div>
        {filtered.map((skill, i) => (
          <div
            key={skill.name}
            style={{
              display: 'flex', alignItems: 'center', gap: 14,
              padding: '14px 0',
              borderBottom: i < filtered.length - 1 ? '1px solid var(--border-subtle)' : 'none',
            }}
          >
            {/* 图标 */}
            <div style={{
              width: 40, height: 40, borderRadius: 10,
              backgroundColor: 'var(--bg-glass)', display: 'flex',
              alignItems: 'center', justifyContent: 'center', fontSize: 20, flexShrink: 0,
            }}>
              {skill.icon}
            </div>

            {/* 名称 + 描述 */}
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)' }}>
                  {skill.name}
                </span>
                {skill.isBuiltin && (
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 4,
                    backgroundColor: '#6366F1', color: '#fff', fontWeight: 600,
                  }}>{t('skills.labelBuiltin')}</span>
                )}
                {!skill.isBuiltin && skill.installed && (
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 4,
                    backgroundColor: 'var(--success)', color: '#fff', fontWeight: 600,
                  }}>{t('skills.labelInstalled')}</span>
                )}
                {skill.tools_count > 0 && (
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 4,
                    backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                  }}>{skill.tools_count} {t('skills.labelTools')}</span>
                )}
              </div>
              {skill.desc && (
                <div style={{
                  fontSize: 12, color: 'var(--text-muted)', marginTop: 3,
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                }}>
                  {skill.desc}
                </div>
              )}
            </div>

            {/* 操作按钮 */}
            {!skill.isBuiltin && (
              <div style={{ flexShrink: 0, display: 'flex', gap: 6 }}>
                {skill.installed ? (<>
                  <button
                    onClick={() => handlePublishToHub(skill.name)}
                    disabled={publishing === skill.name}
                    style={{
                      padding: '5px 10px', borderRadius: 6, fontSize: 11, cursor: 'pointer',
                      border: '1px solid var(--border-subtle)', backgroundColor: 'transparent',
                      color: 'var(--accent)', fontWeight: 500,
                    }}
                  >
                    {publishing === skill.name ? '...' : t('skills.btnPublish')}
                  </button>
                  <button
                    onClick={() => handleUninstall(skill.name)}
                    disabled={operating === skill.name}
                    style={{
                      padding: '5px 10px', borderRadius: 6, fontSize: 11, cursor: 'pointer',
                      border: '1px solid var(--border-subtle)', backgroundColor: 'transparent',
                      color: 'var(--error)', fontWeight: 500,
                    }}
                  >
                    {operating === skill.name ? '...' : t('skills.btnUninstall')}
                  </button>
                </>) : (
                  <button
                    onClick={() => handleInstall(skill.name)}
                    disabled={operating === skill.name}
                    style={{
                      padding: '5px 12px', borderRadius: 6, fontSize: 12, cursor: 'pointer',
                      border: 'none', backgroundColor: 'var(--accent)', color: '#fff', fontWeight: 500,
                    }}
                  >
                    {operating === skill.name ? t('skills.btnInstalling') : t('skills.btnInstall')}
                  </button>
                )}
              </div>
            )}
          </div>
        ))}

        {filtered.length === 0 && activeTab !== 'online' && (
          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>
            {t('skills.emptyCategory')}
          </div>
        )}
      </div>

      {/* 在线市场 */}
      {activeTab === 'online' && (
        <div style={{ marginTop: 16 }}>
          <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
            <input
              value={onlineSearch}
              onChange={e => setOnlineSearch(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) loadOnlineSkills(onlineSearch) }}
              placeholder={t('skills.searchOnline')}
              style={{ flex: 1, padding: '8px 12px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
            />
            <button onClick={() => loadOnlineSkills(onlineSearch)}
              style={{ padding: '8px 16px', borderRadius: 6, backgroundColor: 'var(--accent)', color: '#fff', border: 'none', fontSize: 13, cursor: 'pointer' }}>
              {t('common.search')}
            </button>
          </div>

          {onlineLoading ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('common.loading')}</div>
          ) : onlineSkills.length === 0 ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('skills.emptyOnline')}</div>
          ) : (
            onlineSkills.map((s: any) => (
              <div key={s.slug} style={{
                display: 'flex', alignItems: 'center', gap: 14, padding: '14px 0',
                borderBottom: '1px solid var(--border-subtle)',
              }}>
                <div style={{
                  width: 40, height: 40, borderRadius: 10, backgroundColor: 'var(--bg-glass)',
                  display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 20, flexShrink: 0,
                }}>{s.icon || '🧩'}</div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <span style={{ fontSize: 14, fontWeight: 600 }}>{s.name}</span>
                    <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 4, backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)' }}>{s.category}</span>
                    <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>⬇ {s.downloads}</span>
                  </div>
                  <div style={{ fontSize: 12, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {s.description}
                  </div>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
                  <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>v{s.version}</span>
                  {installedNames.has(s.slug) || marketplace.some(m => m.name === s.slug) ? (
                    <span style={{ fontSize: 11, color: 'var(--success)', fontWeight: 500 }}>{t('skills.labelHas')}</span>
                  ) : (
                    <button
                      onClick={() => handleDownloadFromHub(s.slug)}
                      disabled={downloading === s.slug}
                      style={{
                        padding: '4px 12px', borderRadius: 6, fontSize: 11, cursor: 'pointer',
                        border: 'none', backgroundColor: 'var(--accent)', color: '#fff', fontWeight: 500,
                      }}
                    >
                      {downloading === s.slug ? t('skills.btnDownloading') : t('skills.btnDownload')}
                    </button>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      )}

      {/* 底部提示 */}
      <div style={{
        marginTop: 24, padding: '12px 0', borderTop: '1px solid var(--border-subtle)',
        fontSize: 12, color: 'var(--text-muted)',
      }}>
        {t('skills.hintBottom')}
      </div>
    </div>
  )
}
