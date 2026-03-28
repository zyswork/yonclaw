/**
 * 通用 Modal 弹窗组件
 */
import React from 'react'

interface ModalProps {
  open: boolean
  onClose: () => void
  title?: string
  width?: number
  children: React.ReactNode
  footer?: React.ReactNode
}

export default function Modal({ open, onClose, title, width = 420, children, footer }: ModalProps) {
  if (!open) return null
  return (
    <div style={{
      position: 'fixed', inset: 0, backgroundColor: 'rgba(0,0,0,0.4)',
      display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 100,
    }} onClick={onClose}>
      <div style={{
        backgroundColor: 'var(--bg-elevated)', borderRadius: 12, padding: 24,
        width, maxHeight: '80vh', overflowY: 'auto',
        border: '1px solid var(--border-subtle)',
        boxShadow: '0 20px 60px rgba(0,0,0,0.3)',
        color: 'var(--text-primary)',
      }} onClick={e => e.stopPropagation()}>
        {title && (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 16 }}>
            <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>{title}</h3>
            <button onClick={onClose} style={{ background: 'none', border: 'none', fontSize: 18, cursor: 'pointer', color: 'var(--text-muted)', padding: '0 4px' }}>×</button>
          </div>
        )}
        {children}
        {footer && <div style={{ marginTop: 16, display: 'flex', gap: 8, justifyContent: 'flex-end' }}>{footer}</div>}
      </div>
    </div>
  )
}
