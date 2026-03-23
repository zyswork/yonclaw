/**
 * 工具审批弹窗 Hook
 *
 * 监听 tool-approval-request 事件，弹出审批确认框。
 * 用户点击批准/拒绝后调用 Tauri command。
 */

import { useEffect, useState, useCallback } from 'react'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/tauri'

interface ApprovalRequest {
  requestId: string
  agentId: string
  sessionId: string
  toolName: string
  arguments: Record<string, unknown>
  safetyLevel: string
  timestamp: number
}

export function useApproval() {
  const [pending, setPending] = useState<ApprovalRequest | null>(null)

  useEffect(() => {
    const unlisten = listen<ApprovalRequest>('tool-approval-request', (event) => {
      setPending(event.payload)
    })
    return () => { unlisten.then(fn => fn()) }
  }, [])

  const approve = useCallback(async () => {
    if (!pending) return
    try {
      await invoke('approve_tool_call', { requestId: pending.requestId })
    } catch (e) {
      console.error('Approve failed:', e)
    }
    setPending(null)
  }, [pending])

  const deny = useCallback(async (reason?: string) => {
    if (!pending) return
    try {
      await invoke('deny_tool_call', { requestId: pending.requestId, reason: reason || '' })
    } catch (e) {
      console.error('Deny failed:', e)
    }
    setPending(null)
  }, [pending])

  return { pending, approve, deny }
}

/**
 * 审批弹窗组件 — 放在 App.tsx 根级
 */
export function ApprovalDialog() {
  const { pending, approve, deny } = useApproval()

  if (!pending) return null

  const argsPreview = JSON.stringify(pending.arguments, null, 2)
    .slice(0, 500)

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 10000,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      backgroundColor: 'rgba(0,0,0,0.5)',
      backdropFilter: 'blur(4px)',
    }}>
      <div style={{
        backgroundColor: 'var(--bg-elevated)',
        borderRadius: 16,
        padding: '24px 28px',
        maxWidth: 480,
        width: '90%',
        boxShadow: '0 20px 60px rgba(0,0,0,0.3)',
        border: '1px solid var(--border-subtle)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 16 }}>
          <span style={{ fontSize: 24 }}>{'\u26A0\uFE0F'}</span>
          <h3 style={{ margin: 0, fontSize: 16, fontWeight: 600, color: 'var(--text-primary)' }}>
            {'\u5DE5\u5177\u5BA1\u6279'}
          </h3>
        </div>

        <div style={{
          padding: '12px 16px',
          borderRadius: 10,
          backgroundColor: 'var(--warning-bg)',
          border: '1px solid var(--warning)',
          marginBottom: 16,
          fontSize: 13,
          color: 'var(--text-primary)',
        }}>
          Agent \u8BF7\u6C42\u6267\u884C <strong style={{ color: 'var(--warning)' }}>{pending.toolName}</strong>\uFF0C\u8BE5\u64CD\u4F5C\u9700\u8981\u60A8\u7684\u6279\u51C6\u3002
        </div>

        <div style={{
          fontSize: 12,
          color: 'var(--text-muted)',
          marginBottom: 6,
        }}>
          {'\u53C2\u6570\uFF1A'}
        </div>
        <pre style={{
          backgroundColor: 'var(--bg-glass)',
          padding: '10px 14px',
          borderRadius: 8,
          fontSize: 11,
          fontFamily: 'monospace',
          maxHeight: 200,
          overflow: 'auto',
          color: 'var(--text-secondary)',
          border: '1px solid var(--border-subtle)',
          marginBottom: 20,
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-all',
        }}>
          {argsPreview}
        </pre>

        <div style={{ display: 'flex', gap: 10, justifyContent: 'flex-end' }}>
          <button
            onClick={() => deny()}
            style={{
              padding: '8px 20px',
              borderRadius: 8,
              border: '1px solid var(--border-subtle)',
              backgroundColor: 'transparent',
              color: 'var(--error)',
              fontSize: 13,
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            {'\u62D2\u7EDD'}
          </button>
          <button
            onClick={approve}
            style={{
              padding: '8px 20px',
              borderRadius: 8,
              border: 'none',
              backgroundColor: 'var(--success)',
              color: '#fff',
              fontSize: 13,
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            {'\u6279\u51C6\u6267\u884C'}
          </button>
        </div>
      </div>
    </div>
  )
}
