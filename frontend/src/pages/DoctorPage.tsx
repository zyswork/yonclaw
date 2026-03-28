/**
 * Doctor 诊断页面
 *
 * 系统健康检查 + 一键修复
 */

import { useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import { useAsyncData } from '../hooks/useAsyncData'

interface DiagResult {
  category: string
  check: string
  status: 'Ok' | 'Warning' | 'Error' | 'Fixed'
  message: string
  auto_fix: string | null
}

const STATUS_ICON: Record<string, string> = {
  Ok: '',
  Warning: '',
  Error: '',
  Fixed: '',
}

const STATUS_COLOR: Record<string, string> = {
  Ok: 'var(--success)',
  Warning: '#f0ad4e',
  Error: 'var(--error)',
  Fixed: 'var(--accent)',
}

export default function DoctorPage() {
  const { t } = useI18n()
  const { data: results, loading: running, refetch: runDiag } = useAsyncData(
    async () => {
      const r = await invoke<DiagResult[]>('run_doctor')
      const issues = r.filter(x => x.status !== 'Ok').length
      if (issues === 0) toast.success('All checks passed')
      else toast.error(`${issues} issue(s) found`)
      return r
    },
    [] as DiagResult[],
  )
  const [fixes, setFixes] = useState<DiagResult[]>([])
  const [fixing, setFixing] = useState(false)

  const runFix = async () => {
    setFixing(true)
    try {
      const f = await invoke<DiagResult[]>('doctor_auto_fix')
      setFixes(f)
      if (f.length > 0) {
        toast.success(`Fixed ${f.length} issue(s)`)
        // 重新诊断
        await runDiag()
      } else {
        toast.success('Nothing to fix')
      }
    } catch (e) { toast.error(String(e)) }
    finally { setFixing(false) }
  }

  const hasFixable = results.some(r => r.auto_fix)
  const categories = [...new Set(results.map(r => r.category))]

  return (
    <div style={{ padding: '24px 32px', maxWidth: 800 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
        <h2 style={{ margin: 0, fontSize: 22, fontWeight: 700 }}>
          {t('doctor.title') || 'System Doctor'}
        </h2>
        <div style={{ display: 'flex', gap: 8 }}>
          <button onClick={runDiag} disabled={running}
            style={{
              padding: '8px 20px', borderRadius: 8, border: 'none',
              backgroundColor: 'var(--accent)', color: '#fff', fontSize: 14,
              cursor: running ? 'wait' : 'pointer', opacity: running ? 0.7 : 1,
            }}>
            {running ? 'Checking...' : (t('doctor.runBtn') || 'Run Diagnostics')}
          </button>
          {hasFixable && (
            <button onClick={runFix} disabled={fixing}
              style={{
                padding: '8px 20px', borderRadius: 8, border: '1px solid var(--accent)',
                backgroundColor: 'transparent', color: 'var(--accent)', fontSize: 14,
                cursor: fixing ? 'wait' : 'pointer',
              }}>
              {fixing ? 'Fixing...' : (t('doctor.fixBtn') || 'Auto Fix')}
            </button>
          )}
        </div>
      </div>

      {results.length === 0 && !running && (
        <div style={{ textAlign: 'center', padding: '60px 0', color: 'var(--text-muted)' }}>
          <div style={{ fontSize: 18, marginBottom: 16, color: 'var(--text-muted)' }}>+</div>
          <div style={{ fontSize: 15 }}>{t('doctor.hint') || 'Click "Run Diagnostics" to check system health'}</div>
        </div>
      )}

      {/* 修复结果 */}
      {fixes.length > 0 && (
        <div style={{
          marginBottom: 20, padding: 16, borderRadius: 10,
          backgroundColor: 'var(--success-bg)', border: '1px solid var(--success)',
        }}>
          <strong>Auto-fix Results:</strong>
          {fixes.map((f, i) => (
            <div key={i} style={{ marginTop: 6, fontSize: 13 }}>
              <span style={{
                display: 'inline-block', width: 8, height: 8, borderRadius: '50%', marginRight: 6,
                backgroundColor: STATUS_COLOR[f.status] || 'var(--text-muted)',
              }} />
              [{f.category}] {f.check}: {f.message}
            </div>
          ))}
        </div>
      )}

      {/* 诊断结果按类别 */}
      {categories.map(cat => (
        <div key={cat} style={{ marginBottom: 20 }}>
          <h3 style={{ fontSize: 15, fontWeight: 600, marginBottom: 8, color: 'var(--text-secondary)' }}>
            {cat}
          </h3>
          <div style={{
            border: '1px solid var(--border-subtle)', borderRadius: 10,
            overflow: 'hidden',
          }}>
            {results.filter(r => r.category === cat).map((r, i) => (
              <div key={i} style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '10px 16px',
                borderBottom: '1px solid var(--border-subtle)',
                backgroundColor: r.status === 'Error' ? 'rgba(239,68,68,0.05)' : 'transparent',
              }}>
                <span style={{
                  width: 8, height: 8, borderRadius: '50%', flexShrink: 0,
                  backgroundColor: STATUS_COLOR[r.status] || 'var(--text-muted)',
                }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>{r.check}</div>
                  <div style={{ fontSize: 12, color: STATUS_COLOR[r.status] || 'var(--text-muted)' }}>
                    {r.message}
                  </div>
                </div>
                {r.auto_fix && (
                  <span style={{
                    fontSize: 10, padding: '2px 6px', borderRadius: 4,
                    backgroundColor: 'var(--accent-bg)', color: 'var(--accent)',
                  }}>
                    auto-fixable
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}
