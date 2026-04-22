/**
 * 技能广场 — 深色毛玻璃卡片式布局
 *
 * - 从 ~/.xianzhu/marketplace/ 加载全局可用技能
 * - 从 agent skills/ 加载已安装技能
 * - 支持给指定 Agent 安装/卸载技能
 */

import { useEffect, useState, type CSSProperties } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import { useConfirm } from '../hooks/useConfirm'

interface MarketplaceSkill {
  name: string
  dir_name?: string
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

interface OnlineSkill {
  slug: string
  name: string
  description: string
  category: string
  version: string
  icon: string
  downloads: number
  author?: string
}

/* ─── SVG 图标 ─────────────────────────────── */

function SvgIcon({ name, size = 20, color }: { name: string; size?: number; color?: string }) {
  const c = color || 'currentColor'
  const props = { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: c, strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const }

  switch (name) {
    case 'puzzle':
      return <svg {...props}><path d="M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 01-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 10-3.214 3.214c.446.166.855.497.925.968a.979.979 0 01-.276.837l-1.61 1.61a2.404 2.404 0 01-1.705.707 2.402 2.402 0 01-1.704-.706l-1.568-1.568a1.026 1.026 0 00-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 11-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 1.026 0 00-.289-.877l-1.568-1.568A2.402 2.402 0 011.998 12c0-.617.236-1.234.706-1.704L4.23 8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.5 2.5 0 103.259-3.259c-.482-.196-.933-.558-1.01-1.073-.05-.336.062-.676.303-.917l1.525-1.525A2.402 2.402 0 0112 2c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 1.02-.968a2.5 2.5 0 113.237 3.237c-.464.18-.894.527-.967 1.02z"/></svg>
    case 'search':
      return <svg {...props}><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
    case 'check':
      return <svg {...props}><polyline points="20 6 9 17 4 12"/></svg>
    case 'download':
      return <svg {...props}><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
    case 'upload':
      return <svg {...props}><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
    case 'trash':
      return <svg {...props}><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>
    case 'empty':
      return (
        <svg width={size} height={size} viewBox="0 0 64 64" fill="none">
          <circle cx="32" cy="32" r="28" stroke="var(--border-default)" strokeWidth="2" strokeDasharray="4 4"/>
          <path d="M20 28l6 6-6 6M28 42h12" stroke="var(--text-muted)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
        </svg>
      )
    default:
      return <svg {...props}><circle cx="12" cy="12" r="10"/></svg>
  }
}

// 技能元数据（分类 key）
const SKILL_META: Record<string, { category: string }> = {
  // productivity
  'mail-ops':         { category: 'productivity' },
  'oa-schedule':      { category: 'productivity' },
  'oa-task':          { category: 'productivity' },
  'oa-meeting':       { category: 'productivity' },
  'oa-common':        { category: 'system' },
  'summarize':        { category: 'productivity' },
  'weather':          { category: 'productivity' },
  'apple-notes':      { category: 'productivity' },
  'apple-reminders':  { category: 'productivity' },
  // development
  'github':           { category: 'development' },
  'coding-agent':     { category: 'development' },
  'skill-creator':    { category: 'development' },
  'spec-generator':   { category: 'development' },
  'session-logs':     { category: 'development' },
  'tmux':             { category: 'development' },
  // media
  'nano-banana-pro':  { category: 'media' },
  'nano-pdf':         { category: 'media' },
  'peekaboo':         { category: 'media' },
  // platform
  'clawhub':          { category: 'platform' },
}

// 内置工具（始终可用，不需安装）
const BUILTIN_TOOLS = [
  { name: 'memory_write', descKey: 'skills.builtinMemoryWrite' },
  { name: 'memory_read', descKey: 'skills.builtinMemoryRead' },
  { name: 'bash_exec', descKey: 'skills.builtinBashExec' },
  { name: 'file_read', descKey: 'skills.builtinFileRead' },
  { name: 'file_write', descKey: 'skills.builtinFileWrite' },
  { name: 'file_edit', descKey: 'skills.builtinFileEdit' },
  { name: 'file_list', descKey: 'skills.builtinFileList' },
  { name: 'code_search', descKey: 'skills.builtinCodeSearch' },
  { name: 'web_fetch', descKey: 'skills.builtinWebFetch' },
  { name: 'calculator', descKey: 'skills.builtinCalculator' },
  { name: 'date_time', descKey: 'skills.builtinDateTime' },
  { name: 'diff_edit', descKey: 'skills.builtinDiffEdit' },
  { name: 'settings_manage', descKey: 'skills.builtinSettingsManage' },
  { name: 'provider_manage', descKey: 'skills.builtinProviderManage' },
  { name: 'agent_self_config', descKey: 'skills.builtinAgentSelfConfig' },
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

/* ─── 样式常量 ─────────────────────────────── */

const CARD_STYLE: CSSProperties = {
  background: 'var(--bg-elevated)',
  border: '1px solid var(--border-subtle)',
  borderRadius: 12,
  backdropFilter: 'blur(var(--glass-blur))',
  transition: 'transform 0.2s ease, box-shadow 0.2s ease',
  boxShadow: '0 2px 8px rgba(0,0,0,0.15)',
}

/* ─── 主组件 ─────────────────────────────── */

export default function SkillsPage() {
  const { t } = useI18n()
  const confirm = useConfirm()
  const [marketplace, setMarketplace] = useState<MarketplaceSkill[]>([])
  const [installed, setInstalled] = useState<InstalledSkill[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [activeTab, setActiveTab] = useState('all')
  const [loading, setLoading] = useState(true)
  const [operating, setOperating] = useState('')
  const [onlineSkills, setOnlineSkills] = useState<OnlineSkill[]>([])
  const [onlineSearch, setOnlineSearch] = useState('')
  const [onlineLoading, setOnlineLoading] = useState(false)
  const [downloading, setDownloading] = useState('')
  const [publishing, setPublishing] = useState('')
  const [searchQuery, setSearchQuery] = useState('')
  const [hoveredCard, setHoveredCard] = useState<string | null>(null)

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { if (selectedAgent) loadAll() }, [selectedAgent])
  useEffect(() => { if (activeTab === 'online') loadOnlineSkills() }, [activeTab])

  const loadAgents = async () => {
    try {
      const list = (await invoke('list_agents')) as Array<{ id: string; name: string }>
      setAgents(list.map((a) => ({ id: a.id, name: a.name })))
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
      const resp = await invoke<{ skills?: OnlineSkill[] }>('cloud_api_proxy', { method: 'GET', path, body: null })
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
      toast.success(msg)
    } catch (e) { toast.error(t('skills.downloadFailed') + ': ' + e) }
    setDownloading('')
  }

  const handlePublishToHub = async (skillName: string) => {
    const author = prompt(t('skills.promptAuthor'), '') || ''
    setPublishing(skillName)
    try {
      const msg = await invoke<string>('publish_skill_to_hub', { skillName, author })
      toast.success(msg)
      if (activeTab === 'online') loadOnlineSkills(onlineSearch)
    } catch (e) { toast.error(t('skills.publishFailed') + ': ' + e) }
    setPublishing('')
  }

  const installedNames = new Set(installed.map(s => s.name))

  const handleInstall = async (skillName: string) => {
    setOperating(skillName)
    try {
      const msg = await invoke<string>('install_skill_to_agent', { agentId: selectedAgent, skillName })
      await loadAll()
      // 显示安装成功 + 可能的配置提示
      if (msg && typeof msg === 'string' && msg.length > 0) {
        toast.info(msg)
      } else {
        toast.success(t('skills.installSuccess'))
      }
    } catch (e) { toast.error(t('skills.installFailed') + ': ' + e) }
    setOperating('')
  }

  const handleUninstall = async (skillName: string) => {
    if (!await confirm(`${t('skills.btnUninstall')} ${skillName}?`)) return
    setOperating(skillName)
    try {
      await invoke('uninstall_skill_from_agent', { agentId: selectedAgent, skillName })
      await loadAll()
    } catch (e) { toast.error(t('skills.uninstallFailed') + ': ' + e) }
    setOperating('')
  }

  // 构建统一列表
  type DisplaySkill = {
    name: string; dirName: string; desc: string; category: string
    installed: boolean; isBuiltin: boolean; tools_count: number
  }

  // marketplace 去重（按 dir_name 优先）
  const seenDirs = new Set<string>()
  const dedupedMarketplace = marketplace.filter(s => {
    const key = s.dir_name || s.name
    if (seenDirs.has(key)) return false
    seenDirs.add(key)
    return true
  })

  const allSkills: DisplaySkill[] = [
    // 技能市场的技能（dir_name 是文件系统目录名，用于安装/卸载）
    ...dedupedMarketplace.map(s => {
      const dirName = s.dir_name || s.name
      const meta = SKILL_META[dirName] || SKILL_META[s.name] || { category: 'other' }
      return {
        name: s.name,
        dirName,
        desc: s.description || '',
        category: meta.category,
        installed: installedNames.has(dirName) || installedNames.has(s.name),
        isBuiltin: false,
        tools_count: s.tools_count,
      }
    }),
    // 已安装但不在 marketplace 的技能（用 dir_name 和 name 双重匹配去重）
    ...installed
      .filter(s => !marketplace.some(m => (m.dir_name || m.name) === s.name || m.name === s.name || m.dir_name === s.name) && !BUILTIN_TOOLS.some(b => b.name === s.name))
      .map(s => {
        const meta = SKILL_META[s.name] || { category: 'other' }
        return {
          name: s.name, dirName: s.name, desc: s.description || '',
          category: meta.category, installed: true, isBuiltin: false, tools_count: 0,
        }
      }),
    // 内置工具
    ...BUILTIN_TOOLS.map(b => ({
      name: b.name, dirName: b.name, desc: t(b.descKey), category: 'builtin',
      installed: true, isBuiltin: true, tools_count: 0,
    })),
  ]

  const tabFiltered = activeTab === 'all' ? allSkills.filter(s => !s.isBuiltin)
    : activeTab === 'installed' ? allSkills.filter(s => s.installed && !s.isBuiltin)
    : activeTab === 'available' ? allSkills.filter(s => !s.installed && !s.isBuiltin)
    : activeTab === 'builtin' ? allSkills.filter(s => s.isBuiltin)
    : allSkills.filter(s => s.category === activeTab)

  // 搜索过滤
  const q = searchQuery.toLowerCase().trim()
  const filtered = q
    ? tabFiltered.filter(s => s.name.toLowerCase().includes(q) || s.desc.toLowerCase().includes(q) || s.dirName.toLowerCase().includes(q))
    : tabFiltered

  if (loading) return (
    <div style={{ padding: 40, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
      {t('common.loading')}
    </div>
  )

  const installedCount = allSkills.filter(s => s.installed && !s.isBuiltin).length
  const availableCount = allSkills.filter(s => !s.installed && !s.isBuiltin).length

  return (
    <div style={{ padding: '24px 32px', maxWidth: 960 }}>
      {/* 标题栏（装饰色用薰衣草，不抢 CTA 暖橙） */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 24 }}>
        <div style={{
          width: 42, height: 42, borderRadius: 12,
          backgroundColor: 'rgba(216, 206, 228, 0.28)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}>
          <SvgIcon name="puzzle" size={22} color="var(--accent-2-text)" />
        </div>
        <div>
          <h1 style={{
            margin: 0, fontSize: 22, fontWeight: 700,
            color: 'var(--text-heading)',
          }}>
            {t('skills.title')}
          </h1>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>
            {t('skills.countSkills', { count: marketplace.length })}
          </span>
        </div>
        <span style={{ flex: 1 }} />
        {/* Agent 选择器 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{t('skills.installTo')}</span>
          <select
            value={selectedAgent}
            onChange={(e) => setSelectedAgent(e.target.value)}
            style={{
              padding: '6px 12px', borderRadius: 10, fontSize: 13,
              border: '1px solid var(--border-subtle)',
              backgroundColor: 'var(--bg-glass)', color: 'var(--text-primary)',
            }}
          >
            {agents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
        </div>
      </div>

      {/* 搜索框 */}
      <div style={{ position: 'relative', marginBottom: 20 }}>
        <div style={{
          position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)',
          color: 'var(--text-muted)', display: 'flex', alignItems: 'center',
        }}>
          <SvgIcon name="search" size={16} />
        </div>
        <input
          type="text"
          placeholder={t('skills.searchPlaceholder')}
          value={searchQuery}
          onChange={e => setSearchQuery(e.target.value)}
          style={{
            width: '100%', padding: '10px 14px 10px 40px',
            borderRadius: 10, border: '1px solid var(--border-subtle)',
            fontSize: 13, backgroundColor: 'var(--bg-elevated)',
            color: 'var(--text-primary)', boxSizing: 'border-box',
            outline: 'none', transition: 'border-color 0.2s ease',
          }}
        />
      </div>

      {/* 分类 Tab — pill 样式 */}
      <div style={{
        display: 'inline-flex', gap: 4, marginBottom: 24, flexWrap: 'wrap',
        padding: '4px', borderRadius: 12,
        backgroundColor: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
      }}>
        {CATEGORY_KEYS.map(cat => {
          const count = cat === 'installed' ? installedCount : cat === 'available' ? availableCount : undefined
          const isActive = activeTab === cat
          return (
            <button
              key={cat}
              onClick={() => setActiveTab(cat)}
              style={{
                padding: '6px 16px', borderRadius: 10, fontSize: 12, cursor: 'pointer',
                backgroundColor: isActive ? 'var(--accent-2-bg)' : 'transparent',
                color: isActive ? 'var(--text-heading)' : 'var(--text-secondary)',
                border: isActive ? '1px solid var(--accent-2)' : '1px solid transparent',
                fontWeight: isActive ? 600 : 400,
                transition: 'all 0.2s ease',
              }}
            >
              {t(CATEGORY_I18N[cat] || cat)}{count !== undefined ? ` ${count}` : ''}
            </button>
          )
        })}
      </div>

      {/* 技能卡片网格 */}
      {activeTab !== 'online' && (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 12 }}>
          {filtered.map((skill) => {
            const isHovered = hoveredCard === skill.name
            return (
              <div
                key={skill.name}
                onMouseEnter={() => setHoveredCard(skill.name)}
                onMouseLeave={() => setHoveredCard(null)}
                style={{
                  ...CARD_STYLE,
                  padding: '16px 18px',
                  transform: isHovered ? 'translateY(-2px)' : 'none',
                  boxShadow: isHovered ? '0 8px 24px rgba(0,0,0,0.25)' : '0 2px 8px rgba(0,0,0,0.15)',
                }}
              >
                {/* 上部：图标 + 名称 + 标签 */}
                <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12, marginBottom: 10 }}>
                  <div style={{
                    width: 40, height: 40, borderRadius: 10, flexShrink: 0,
                    backgroundColor: 'rgba(216, 206, 228, 0.22)',
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                  }}>
                    <SvgIcon name="puzzle" size={20} color="var(--accent-2-text)" />
                  </div>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                      <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)' }}>
                        {skill.name}
                      </span>
                      {skill.isBuiltin && (
                        <span style={{
                          fontSize: 10, padding: '1px 6px', borderRadius: 6,
                          backgroundColor: '#6366F1', color: '#fff', fontWeight: 600,
                        }}>{t('skills.labelBuiltin')}</span>
                      )}
                      {!skill.isBuiltin && skill.installed && (
                        <span style={{
                          display: 'inline-flex', alignItems: 'center', gap: 2,
                          fontSize: 10, padding: '1px 6px', borderRadius: 6,
                          backgroundColor: 'var(--success-bg)', color: 'var(--success)', fontWeight: 600,
                        }}>
                          <SvgIcon name="check" size={10} color="var(--success)" />
                          {t('skills.labelInstalled')}
                        </span>
                      )}
                    </div>
                    {skill.tools_count > 0 && (
                      <span style={{
                        fontSize: 10, color: 'var(--text-muted)',
                      }}>
                        {skill.tools_count} {t('skills.labelTools')}
                      </span>
                    )}
                  </div>
                </div>

                {/* 描述 */}
                {skill.desc && (
                  <div style={{
                    fontSize: 12, color: 'var(--text-muted)', marginBottom: 12,
                    lineHeight: 1.5,
                    display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
                    overflow: 'hidden',
                  }}>
                    {skill.desc}
                  </div>
                )}

                {/* 操作按钮 */}
                {!skill.isBuiltin && (
                  <div style={{ display: 'flex', gap: 6 }}>
                    {skill.installed ? (<>
                      {/* 发布按钮仅对本地自建技能显示（不在市场中的） */}
                      {!dedupedMarketplace.some(m => (m.dir_name || m.name) === skill.dirName || m.name === skill.name) && (
                        <button
                          onClick={() => handlePublishToHub(skill.dirName)}
                          disabled={publishing === skill.dirName}
                          style={{
                            display: 'flex', alignItems: 'center', gap: 4,
                            padding: '6px 12px', borderRadius: 8, fontSize: 11, cursor: 'pointer',
                            border: '1px solid var(--border-subtle)', backgroundColor: 'transparent',
                            color: 'var(--text-accent)', fontWeight: 500, transition: 'all 0.15s ease',
                          }}
                        >
                          <SvgIcon name="upload" size={12} color="var(--text-accent)" />
                          {publishing === skill.dirName ? '...' : t('skills.btnPublish')}
                        </button>
                      )}
                      <button
                        onClick={() => handleUninstall(skill.dirName)}
                        disabled={operating === skill.dirName}
                        style={{
                          display: 'flex', alignItems: 'center', gap: 4,
                          padding: '6px 12px', borderRadius: 8, fontSize: 11, cursor: 'pointer',
                          border: '1px solid var(--border-subtle)', backgroundColor: 'transparent',
                          color: 'var(--error)', fontWeight: 500, transition: 'all 0.15s ease',
                        }}
                      >
                        <SvgIcon name="trash" size={12} color="var(--error)" />
                        {operating === skill.dirName ? '...' : t('skills.btnUninstall')}
                      </button>
                    </>) : (
                      <button
                        onClick={() => handleInstall(skill.dirName)}
                        disabled={operating === skill.dirName}
                        style={{
                          display: 'flex', alignItems: 'center', gap: 4,
                          padding: '6px 14px', borderRadius: 8, fontSize: 12, cursor: 'pointer',
                          fontWeight: 500, transition: 'all 0.15s ease',
                          backgroundColor: 'transparent',
                          color: 'var(--accent-2-text)',
                          border: '1px solid var(--accent-2)',
                        }}
                      >
                        <SvgIcon name="download" size={13} color="var(--accent-2-text)" />
                        {operating === skill.dirName ? t('skills.btnInstalling') : t('skills.btnInstall')}
                      </button>
                    )}
                  </div>
                )}
              </div>
            )
          })}

          {filtered.length === 0 && (
            <div style={{
              gridColumn: '1 / -1',
              ...CARD_STYLE,
              padding: '48px 24px',
              display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 16,
            }}>
              <SvgIcon name="empty" size={64} />
              <div style={{ color: 'var(--text-muted)', fontSize: 14 }}>
                {t('skills.emptyCategory')}
              </div>
            </div>
          )}
        </div>
      )}

      {/* 在线市场 */}
      {activeTab === 'online' && (
        <div>
          <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
            <div style={{ flex: 1, position: 'relative' }}>
              <div style={{
                position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)',
                color: 'var(--text-muted)', display: 'flex', alignItems: 'center',
              }}>
                <SvgIcon name="search" size={16} />
              </div>
              <input
                value={onlineSearch}
                onChange={e => setOnlineSearch(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) loadOnlineSkills(onlineSearch) }}
                placeholder={t('skills.searchOnline')}
                style={{
                  width: '100%', padding: '10px 14px 10px 40px',
                  borderRadius: 10, border: '1px solid var(--border-subtle)',
                  fontSize: 13, backgroundColor: 'var(--bg-elevated)',
                  color: 'var(--text-primary)', boxSizing: 'border-box',
                  outline: 'none',
                }}
              />
            </div>
            <button onClick={() => loadOnlineSkills(onlineSearch)}
              style={{
                padding: '8px 20px', borderRadius: 10, fontSize: 13, cursor: 'pointer',
                backgroundColor: 'transparent', color: 'var(--accent-2-text)',
                border: '1px solid var(--accent-2)', fontWeight: 500,
              }}>
              {t('common.search')}
            </button>
          </div>

          {onlineLoading ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>{t('common.loading')}</div>
          ) : onlineSkills.length === 0 ? (
            <div style={{
              ...CARD_STYLE, padding: '48px 24px',
              display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 16,
            }}>
              <SvgIcon name="empty" size={64} />
              <div style={{ color: 'var(--text-muted)', fontSize: 14 }}>
                {t('skills.emptyOnline')}
              </div>
            </div>
          ) : (
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 12 }}>
              {onlineSkills.map((s) => {
                const isHovered = hoveredCard === `online-${s.slug}`
                const alreadyHas = installedNames.has(s.slug) || marketplace.some(m => m.name === s.slug)
                return (
                  <div
                    key={s.slug}
                    onMouseEnter={() => setHoveredCard(`online-${s.slug}`)}
                    onMouseLeave={() => setHoveredCard(null)}
                    style={{
                      ...CARD_STYLE,
                      padding: '16px 18px',
                      transform: isHovered ? 'translateY(-2px)' : 'none',
                      boxShadow: isHovered ? '0 8px 24px rgba(0,0,0,0.25)' : '0 2px 8px rgba(0,0,0,0.15)',
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12, marginBottom: 10 }}>
                      <div style={{
                        width: 40, height: 40, borderRadius: 10, flexShrink: 0,
                        backgroundColor: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
                        display: 'flex', alignItems: 'center', justifyContent: 'center',
                      }}>
                        {s.icon ? (
                          <span style={{ fontSize: 20 }}>{s.icon}</span>
                        ) : (
                          <SvgIcon name="puzzle" size={20} color="var(--accent-2)" />
                        )}
                      </div>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                          <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)' }}>{s.name}</span>
                          <span style={{
                            fontSize: 10, padding: '1px 6px', borderRadius: 6,
                            backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                          }}>{s.category}</span>
                        </div>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2 }}>
                          <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>v{s.version}</span>
                          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 2, fontSize: 10, color: 'var(--text-muted)' }}>
                            <SvgIcon name="download" size={10} color="var(--text-muted)" /> {s.downloads}
                          </span>
                        </div>
                      </div>
                    </div>
                    <div style={{
                      fontSize: 12, color: 'var(--text-muted)', marginBottom: 12,
                      lineHeight: 1.5,
                      display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
                      overflow: 'hidden',
                    }}>
                      {s.description}
                    </div>
                    <div>
                      {alreadyHas ? (
                        <span style={{
                          display: 'inline-flex', alignItems: 'center', gap: 4,
                          fontSize: 12, color: 'var(--success)', fontWeight: 600,
                        }}>
                          <SvgIcon name="check" size={14} color="var(--success)" />
                          {t('skills.labelHas')}
                        </span>
                      ) : (
                        <button
                          onClick={() => handleDownloadFromHub(s.slug)}
                          disabled={downloading === s.slug}
                          style={{
                            display: 'flex', alignItems: 'center', gap: 4,
                            padding: '6px 14px', borderRadius: 8, fontSize: 12, cursor: 'pointer',
                            fontWeight: 500,
                            backgroundColor: 'transparent', color: 'var(--accent-2-text)',
                            border: '1px solid var(--accent-2)',
                          }}
                        >
                          <SvgIcon name="download" size={13} color="#fff" />
                          {downloading === s.slug ? t('skills.btnDownloading') : t('skills.btnDownload')}
                        </button>
                      )}
                    </div>
                  </div>
                )
              })}
            </div>
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
