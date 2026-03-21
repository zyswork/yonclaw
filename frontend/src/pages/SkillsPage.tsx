/**
 * 技能广场 — Marketplace + 多 Agent 安装管理
 *
 * - 从 ~/.yonclaw/marketplace/ 加载全局可用技能
 * - 从 agent skills/ 加载已安装技能
 * - 支持给指定 Agent 安装/卸载技能
 */

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'

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

// 技能元数据（icon + 分类）
const SKILL_META: Record<string, { icon: string; category: string }> = {
  // 生产力
  'mail-ops':         { icon: '\u{1F4E8}', category: '生产力' },
  'oa-schedule':      { icon: '\u{1F4C5}', category: '生产力' },
  'oa-task':          { icon: '\u2705',     category: '生产力' },
  'oa-meeting':       { icon: '\u{1F4C5}', category: '生产力' },
  'oa-common':        { icon: '\u{1F527}', category: '系统' },
  'summarize':        { icon: '\u{1F4DD}', category: '生产力' },
  'weather':          { icon: '\u26C5',     category: '生产力' },
  'apple-notes':      { icon: '\u{1F4D3}', category: '生产力' },
  'apple-reminders':  { icon: '\u{1F514}', category: '生产力' },
  // 开发
  'github':           { icon: '\u{1F4BB}', category: '开发' },
  'coding-agent':     { icon: '\u{1F916}', category: '开发' },
  'skill-creator':    { icon: '\u2728',     category: '开发' },
  'spec-generator':   { icon: '\u{1F4CB}', category: '开发' },
  'session-logs':     { icon: '\u{1F4DC}', category: '开发' },
  'tmux':             { icon: '\u{1F5A5}\uFE0F', category: '开发' },
  // 媒体
  'nano-banana-pro':  { icon: '\u{1F3A8}', category: '媒体' },
  'nano-pdf':         { icon: '\u{1F4C4}', category: '媒体' },
  'peekaboo':         { icon: '\u{1F441}\uFE0F', category: '媒体' },
  // 平台
  'clawhub':          { icon: '\u{1F30D}', category: '平台' },
}

// 内置工具（始终可用，不需安装）
const BUILTIN_TOOLS = [
  { name: 'memory_write', desc: '长期记忆存储', icon: '\u{1F9E0}' },
  { name: 'memory_read', desc: '记忆检索', icon: '\u{1F50D}' },
  { name: 'bash_exec', desc: '执行终端命令', icon: '\u{1F4BB}' },
  { name: 'file_read', desc: '读取文件', icon: '\u{1F4C4}' },
  { name: 'file_write', desc: '写入文件', icon: '\u{1F4DD}' },
  { name: 'file_edit', desc: '编辑文件', icon: '\u270F\uFE0F' },
  { name: 'file_list', desc: '列出目录', icon: '\u{1F4C1}' },
  { name: 'code_search', desc: '代码搜索', icon: '\u{1F50E}' },
  { name: 'web_fetch', desc: '获取网页', icon: '\u{1F310}' },
  { name: 'calculator', desc: '数学计算', icon: '\u{1F522}' },
  { name: 'date_time', desc: '当前时间', icon: '\u{1F552}' },
  { name: 'diff_edit', desc: 'Diff 编辑', icon: '\u{1F4CB}' },
  { name: 'settings_manage', desc: '系统设置', icon: '\u{1F527}' },
  { name: 'provider_manage', desc: '供应商管理', icon: '\u2699\uFE0F' },
  { name: 'agent_self_config', desc: '自身配置', icon: '\u{1F916}' },
]

const CATEGORIES = ['全部', '已安装', '可安装', '在线市场', '生产力', '开发', '媒体', '平台', '内置工具']

export default function SkillsPage() {
  const [marketplace, setMarketplace] = useState<MarketplaceSkill[]>([])
  const [installed, setInstalled] = useState<InstalledSkill[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [selectedAgent, setSelectedAgent] = useState('')
  const [activeTab, setActiveTab] = useState('全部')
  const [loading, setLoading] = useState(true)
  const [operating, setOperating] = useState('')
  const [onlineSkills, setOnlineSkills] = useState<any[]>([])
  const [onlineSearch, setOnlineSearch] = useState('')
  const [onlineLoading, setOnlineLoading] = useState(false)

  useEffect(() => { loadAgents() }, [])
  useEffect(() => { if (selectedAgent) loadAll() }, [selectedAgent])
  useEffect(() => { if (activeTab === '在线市场') loadOnlineSkills() }, [activeTab])

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
      const url = q
        ? `https://zys-openclaw.com/api/v1/skill-hub/search?q=${encodeURIComponent(q)}`
        : 'https://zys-openclaw.com/api/v1/skill-hub/search'
      const resp = await invoke<string>('cloud_api_proxy', { url, method: 'GET', body: '' })
      const data = JSON.parse(resp)
      setOnlineSkills(data.skills || [])
    } catch (e) {
      console.error('加载在线技能失败:', e)
      setOnlineSkills([])
    }
    setOnlineLoading(false)
  }

  const installedNames = new Set(installed.map(s => s.name))

  const handleInstall = async (skillName: string) => {
    setOperating(skillName)
    try {
      await invoke('install_skill_to_agent', { agentId: selectedAgent, skillName })
      await loadAll()
    } catch (e) { alert('安装失败: ' + e) }
    setOperating('')
  }

  const handleUninstall = async (skillName: string) => {
    if (!confirm(`确定从当前 Agent 卸载 ${skillName}？`)) return
    setOperating(skillName)
    try {
      await invoke('uninstall_skill_from_agent', { agentId: selectedAgent, skillName })
      await loadAll()
    } catch (e) { alert('卸载失败: ' + e) }
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
      const meta = SKILL_META[s.name] || { icon: '\u{1F9E9}', category: '其他' }
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
        const meta = SKILL_META[s.name] || { icon: '\u{1F9E9}', category: '其他' }
        return {
          name: s.name, desc: s.description || '', icon: meta.icon,
          category: meta.category, installed: true, isBuiltin: false, tools_count: 0,
        }
      }),
    // 内置工具
    ...BUILTIN_TOOLS.map(b => ({
      name: b.name, desc: b.desc, icon: b.icon, category: '内置工具',
      installed: true, isBuiltin: true, tools_count: 0,
    })),
  ]

  const filtered = activeTab === '全部' ? allSkills.filter(s => !s.isBuiltin)
    : activeTab === '已安装' ? allSkills.filter(s => s.installed && !s.isBuiltin)
    : activeTab === '可安装' ? allSkills.filter(s => !s.installed && !s.isBuiltin)
    : activeTab === '内置工具' ? allSkills.filter(s => s.isBuiltin)
    : allSkills.filter(s => s.category === activeTab)

  if (loading) return <div style={{ padding: 24, color: 'var(--text-muted)' }}>加载中...</div>

  const installedCount = allSkills.filter(s => s.installed && !s.isBuiltin).length
  const availableCount = allSkills.filter(s => !s.installed && !s.isBuiltin).length

  return (
    <div style={{ padding: '24px 32px', maxWidth: 900 }}>
      {/* 标题栏 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 20 }}>
        <h1 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>技能广场</h1>
        <span style={{
          fontSize: 12, padding: '2px 8px', borderRadius: 10,
          backgroundColor: 'var(--bg-glass)', color: 'var(--text-secondary)',
        }}>
          {marketplace.length} 个技能
        </span>
        <span style={{ flex: 1 }} />
        {/* Agent 选择器 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>安装到：</span>
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
        {CATEGORIES.map(cat => {
          const count = cat === '已安装' ? installedCount : cat === '可安装' ? availableCount : undefined
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
              {cat}{count !== undefined ? ` ${count}` : ''}
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
                  }}>内置</span>
                )}
                {!skill.isBuiltin && skill.installed && (
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 4,
                    backgroundColor: 'var(--success)', color: '#fff', fontWeight: 600,
                  }}>已安装</span>
                )}
                {skill.tools_count > 0 && (
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 4,
                    backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                  }}>{skill.tools_count} 工具</span>
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
              <div style={{ flexShrink: 0 }}>
                {skill.installed ? (
                  <button
                    onClick={() => handleUninstall(skill.name)}
                    disabled={operating === skill.name}
                    style={{
                      padding: '5px 12px', borderRadius: 6, fontSize: 12, cursor: 'pointer',
                      border: '1px solid var(--border-subtle)', backgroundColor: 'transparent',
                      color: 'var(--error)', fontWeight: 500,
                    }}
                  >
                    {operating === skill.name ? '...' : '卸载'}
                  </button>
                ) : (
                  <button
                    onClick={() => handleInstall(skill.name)}
                    disabled={operating === skill.name}
                    style={{
                      padding: '5px 12px', borderRadius: 6, fontSize: 12, cursor: 'pointer',
                      border: 'none', backgroundColor: 'var(--accent)', color: '#fff', fontWeight: 500,
                    }}
                  >
                    {operating === skill.name ? '安装中...' : '安装'}
                  </button>
                )}
              </div>
            )}
          </div>
        ))}

        {filtered.length === 0 && activeTab !== '在线市场' && (
          <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>
            该分类暂无技能
          </div>
        )}
      </div>

      {/* 在线市场 */}
      {activeTab === '在线市场' && (
        <div style={{ marginTop: 16 }}>
          <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
            <input
              value={onlineSearch}
              onChange={e => setOnlineSearch(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) loadOnlineSkills(onlineSearch) }}
              placeholder="搜索在线技能..."
              style={{ flex: 1, padding: '8px 12px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 13 }}
            />
            <button onClick={() => loadOnlineSkills(onlineSearch)}
              style={{ padding: '8px 16px', borderRadius: 6, backgroundColor: 'var(--accent)', color: '#fff', border: 'none', fontSize: 13, cursor: 'pointer' }}>
              搜索
            </button>
          </div>

          {onlineLoading ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>加载中...</div>
          ) : onlineSkills.length === 0 ? (
            <div style={{ padding: 40, textAlign: 'center', color: 'var(--text-muted)' }}>暂无在线技能</div>
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
                <div style={{ fontSize: 11, color: 'var(--text-muted)', flexShrink: 0 }}>
                  v{s.version} · {s.author}
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
        技能安装到选定的 Agent，不同 Agent 可以有不同的技能组合。安装后 Agent 会在对话中自动使用对应技能。
      </div>
    </div>
  )
}
