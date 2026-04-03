/**
 * 全局 Toast 通知 — 替代 alert()
 *
 * 用法：
 *   const toast = useToast()
 *   toast.success('保存成功')
 *   toast.error('操作失败: ' + err)
 *   toast.info('提示信息')
 */

import { create } from 'zustand'

interface ToastItem {
  id: number
  type: 'success' | 'error' | 'info'
  message: string
}

interface ToastStore {
  items: ToastItem[]
  add: (type: ToastItem['type'], message: string) => void
  remove: (id: number) => void
}

let _id = 0

export const useToastStore = create<ToastStore>((set) => ({
  items: [],
  add: (type, message) => {
    const id = ++_id
    set((s) => ({ items: [...s.items, { id, type, message }] }))
    setTimeout(() => set((s) => ({ items: s.items.filter((i) => i.id !== id) })), 4000)
  },
  remove: (id) => set((s) => ({ items: s.items.filter((i) => i.id !== id) })),
}))

/** Hook 快捷方法 */
export function useToast() {
  const add = useToastStore((s) => s.add)
  return {
    success: (msg: string) => add('success', msg),
    error: (msg: string) => add('error', msg),
    info: (msg: string) => add('info', msg),
  }
}

/** 非组件内使用（事件回调等） */
export const toast = {
  success: (msg: string) => useToastStore.getState().add('success', msg),
  error: (msg: string) => useToastStore.getState().add('error', msg),
  info: (msg: string) => useToastStore.getState().add('info', msg),
}

/** 将原始错误转为用户友好消息 */
export function friendlyError(e: unknown): string {
  const raw = String(e)
  // Tauri invoke 错误通常是 "Unhandled Rejection: ..."
  if (raw.includes('Unhandled')) return '操作失败，请稍后重试'
  // 网络错误
  if (raw.includes('Network Error') || raw.includes('fetch')) return '网络连接失败，请检查网络'
  if (raw.includes('timeout') || raw.includes('Timeout')) return '请求超时，请重试'
  // 权限
  if (raw.includes('Permission') || raw.includes('permission')) return '权限不足'
  // 文件
  if (raw.includes('No such file') || raw.includes('not found')) return '文件不存在'
  // 截断过长的技术错误
  if (raw.length > 100) return raw.slice(0, 100) + '...'
  return raw
}

const COLORS = {
  success: { bg: '#dcfce7', border: '#86efac', color: '#166534' },
  error: { bg: '#fef2f2', border: '#fecaca', color: '#991b1b' },
  info: { bg: '#eff6ff', border: '#bfdbfe', color: '#1e40af' },
}

/** 渲染组件 — 放在 App 最外层 */
export function ToastContainer() {
  const items = useToastStore((s) => s.items)
  const remove = useToastStore((s) => s.remove)

  if (items.length === 0) return null

  return (
    <div style={{
      position: 'fixed', top: 16, right: 16, zIndex: 9999,
      display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 380,
    }}>
      {items.map((item) => {
        const c = COLORS[item.type]
        return (
          <div
            key={item.id}
            onClick={() => remove(item.id)}
            style={{
              padding: '10px 16px', borderRadius: 8,
              backgroundColor: c.bg, border: `1px solid ${c.border}`, color: c.color,
              fontSize: 13, lineHeight: 1.5, cursor: 'pointer',
              boxShadow: '0 4px 12px rgba(0,0,0,0.1)',
              animation: 'toast-in 0.2s ease-out',
              wordBreak: 'break-word',
            }}
          >
            {item.message}
          </div>
        )
      })}
      <style>{`@keyframes toast-in { from { opacity: 0; transform: translateX(20px); } to { opacity: 1; transform: translateX(0); } }`}</style>
    </div>
  )
}
