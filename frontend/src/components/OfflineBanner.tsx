import { useEffect, useState } from 'react'

/**
 * 离线检测 Banner
 *
 * 监听 navigator.onLine + online/offline 事件。
 * 离线时顶部显示黄色横条，恢复后短暂显示绿色"已恢复"后自动消失。
 */
export default function OfflineBanner() {
  const [online, setOnline] = useState(() => navigator.onLine)
  const [justRestored, setJustRestored] = useState(false)

  useEffect(() => {
    const onOnline = () => {
      setOnline(true)
      setJustRestored(true)
      setTimeout(() => setJustRestored(false), 2500)
    }
    const onOffline = () => {
      setOnline(false)
      setJustRestored(false)
    }
    window.addEventListener('online', onOnline)
    window.addEventListener('offline', onOffline)
    return () => {
      window.removeEventListener('online', onOnline)
      window.removeEventListener('offline', onOffline)
    }
  }, [])

  if (online && !justRestored) return null

  const isOffline = !online
  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        // 低于 updater banner（10000），高于普通弹窗。
        // 若两者同时可见，updater 在上覆盖（用户可先处理更新）。
        zIndex: 9998,
        padding: '6px 16px',
        textAlign: 'center',
        fontSize: 13,
        fontWeight: 500,
        color: '#fff',
        backgroundColor: isOffline ? '#f59e0b' : '#10b981',
        boxShadow: '0 2px 8px rgba(0,0,0,0.25)',
        transition: 'background-color 0.3s ease',
      }}
    >
      {isOffline ? (
        <>
          <span style={{ marginRight: 6 }}>⚠️</span>
          网络已断开 — 部分功能（云端 LLM、搜索、OAuth）将不可用
        </>
      ) : (
        <>
          <span style={{ marginRight: 6 }}>✅</span>
          网络已恢复
        </>
      )}
    </div>
  )
}
