/**
 * Layout — 深色侧边栏 + 暖白内容区
 */

import Sidebar from './Sidebar'

export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden' }}>
      <Sidebar />
      <main style={{ flex: 1, minWidth: 0, overflow: 'auto', backgroundColor: 'var(--bg-base)', position: 'relative', paddingTop: 28 }}>
        {/* 可拖拽区域（替代标题栏） */}
        <div data-tauri-drag-region style={{ position: 'fixed', top: 0, left: 210, right: 0, height: 28, zIndex: 50 }} />
        {children}
      </main>
    </div>
  )
}
