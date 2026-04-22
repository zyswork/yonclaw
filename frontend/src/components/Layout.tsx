/**
 * Layout — 深色侧边栏 + 暖白内容区
 * 注册全局键盘快捷键
 */

import { useNavigate } from 'react-router-dom'
import Sidebar from './Sidebar'
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts'
import { useSidebarStore } from '../store/sidebarStore'

/**
 * 侧边栏导航项路径 — Cmd+1~9 快捷键映射
 * 顺序与聚合后的侧栏分组对齐
 */
const NAV_PATHS = [
  '/agents',      // 1: 对话 → 聊天
  '/group-chat',  // 2: 对话 → 群聊
  '/skills',      // 3: 智能扩展 → 技能
  '/plugins',     // 4: 智能扩展 → 插件
  '/memory',      // 5: 智能扩展 → 记忆
  '/cron',        // 6: 自动化 → 定时任务
  '/channels',    // 7: 自动化 → 频道
  '/dashboard',   // 8: 数据洞察 → 仪表板
  '/plaza',       // 9: 数据洞察 → 广场
]

export default function Layout({ children }: { children: React.ReactNode }) {
  const navigate = useNavigate()
  const toggle = useSidebarStore((s) => s.toggle)

  // 构建快捷键映射
  const shortcuts: Record<string, () => void> = {
    // Cmd+, → 打开设置页
    'cmd+,': () => navigate('/settings'),
    // Cmd+Shift+S → 切换侧边栏
    'cmd+shift+s': () => toggle(),
    // Cmd+K → 聚焦搜索框或输入框
    'cmd+k': () => {
      const input = document.querySelector<HTMLElement>(
        'input[type="search"], input[type="text"], textarea'
      )
      input?.focus()
    },
    // Escape → 关闭弹窗（点击 overlay 的模拟）
    'escape': () => {
      const overlay = document.querySelector<HTMLElement>('[style*="position: fixed"][style*="inset: 0"]')
      overlay?.click()
    },
  }

  // Cmd+1~9 → 切换侧边栏导航项
  NAV_PATHS.forEach((path, i) => {
    shortcuts[`cmd+${i + 1}`] = () => navigate(path)
  })

  useKeyboardShortcuts(shortcuts)

  return (
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden' }}>
      <Sidebar />
      {/* macOS 标题栏拖拽区域 — 覆盖整个顶部，支持双击最大化 */}
      <div data-tauri-drag-region style={{ position: 'fixed', top: 0, left: 0, right: 0, height: 34, zIndex: 50, WebkitAppRegion: 'drag' } as any} />
      <main style={{ flex: 1, minWidth: 0, overflow: 'auto', backgroundColor: 'transparent', position: 'relative', paddingTop: 34, WebkitAppRegion: 'no-drag' } as any}>
        {children}
      </main>
    </div>
  )
}
