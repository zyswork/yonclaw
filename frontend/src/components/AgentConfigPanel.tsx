/**
 * Agent 配置面板
 *
 * 右侧滑出面板，包含四个 Tab：
 * - 🎭 Soul: 灵魂文件编辑
 * - 🔧 Tools: 工具管理
 * - ⚙️ Params: 参数配置
 * - 🔌 MCP: MCP Server 管理
 */

import { useState } from 'react'
import { useI18n } from '../i18n'
import SoulFileTab from './SoulFileTab'
import ToolsTab from './ToolsTab'
import ParamsTab from './ParamsTab'
import McpTab from './McpTab'

interface AgentConfigPanelProps {
  agentId: string
  onClose: () => void
}

type TabId = 'soul' | 'tools' | 'params' | 'mcp'

const TABS: { id: TabId; icon: string; label: string }[] = [
  { id: 'soul', icon: '🎭', label: 'Soul' },
  { id: 'tools', icon: '🔧', label: 'Tools' },
  { id: 'params', icon: '⚙️', label: 'Params' },
  { id: 'mcp', icon: '🔌', label: 'MCP' },
]

export default function AgentConfigPanel({ agentId, onClose }: AgentConfigPanelProps) {
  const { t } = useI18n()
  const [activeTab, setActiveTab] = useState<TabId>('soul')

  return (
    <div style={{
      width: '300px', borderLeft: '1px solid #ddd', display: 'flex',
      flexDirection: 'column', backgroundColor: '#fff', flexShrink: 0,
    }}>
      {/* 头部：标题 + 关闭按钮 */}
      <div style={{
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        padding: '8px 12px', borderBottom: '1px solid #eee',
      }}>
        <span style={{ fontSize: '13px', fontWeight: 600, color: '#333' }}>{t('chatPage.configTitle')}</span>
        <button
          onClick={onClose}
          style={{
            background: 'none', border: 'none', cursor: 'pointer',
            fontSize: '16px', color: '#999', padding: '2px 6px', lineHeight: 1,
          }}
          title={t('common.cancel')}
        >
          ✕
        </button>
      </div>

      {/* Tab 切换栏 */}
      <div style={{
        display: 'flex', borderBottom: '1px solid #eee', padding: '0 8px',
      }}>
        {TABS.map(tab => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            style={{
              flex: 1, padding: '8px 4px', border: 'none', cursor: 'pointer',
              fontSize: '12px', display: 'flex', alignItems: 'center',
              justifyContent: 'center', gap: '4px',
              backgroundColor: 'transparent',
              color: activeTab === tab.id ? 'var(--accent)' : '#666',
              borderBottom: activeTab === tab.id ? '2px solid var(--accent)' : '2px solid transparent',
              fontWeight: activeTab === tab.id ? 600 : 400,
            }}
          >
            <span>{tab.icon}</span>
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      {/* Tab 内容区 */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '8px 12px' }}>
        {activeTab === 'soul' && <SoulFileTab agentId={agentId} />}
        {activeTab === 'tools' && <ToolsTab agentId={agentId} />}
        {activeTab === 'params' && <ParamsTab agentId={agentId} />}
        {activeTab === 'mcp' && <McpTab agentId={agentId} />}
      </div>
    </div>
  )
}
