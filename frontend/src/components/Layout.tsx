/**
 * Layout — 深色侧边栏 + 暖白内容区
 */

import Sidebar from './Sidebar'

export default function Layout({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', height: '100vh', overflow: 'hidden' }}>
      <Sidebar />
      <main style={{ flex: 1, minWidth: 0, overflow: 'auto', backgroundColor: 'var(--bg-base)', position: 'relative' }}>
        {children}
      </main>
    </div>
  )
}
