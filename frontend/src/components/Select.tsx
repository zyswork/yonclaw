/**
 * 自定义 Select 下拉框 — 替代原生 <select>，完全可控样式
 * 支持搜索过滤、键盘导航、深色主题
 */
import { useState, useRef, useEffect, useCallback } from 'react'

interface SelectOption {
  value: string
  label: string
  group?: string
}

interface SelectProps {
  value: string
  onChange: (value: string) => void
  options: SelectOption[]
  placeholder?: string
  searchable?: boolean
  style?: React.CSSProperties
  disabled?: boolean
}

export default function Select({ value, onChange, options, placeholder = 'Select...', searchable = false, style, disabled }: SelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [highlightIdx, setHighlightIdx] = useState(-1)
  const containerRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  const selectedOption = options.find(o => o.value === value)

  const filtered = search
    ? options.filter(o => o.label.toLowerCase().includes(search.toLowerCase()) || o.value.toLowerCase().includes(search.toLowerCase()))
    : options

  // 点击外部关闭
  useEffect(() => {
    if (!open) return
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false)
        setSearch('')
      }
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [open])

  // 打开时聚焦搜索框
  useEffect(() => {
    if (open && searchable) {
      setTimeout(() => searchRef.current?.focus(), 50)
    }
    if (open) {
      // 高亮当前选中项
      const idx = filtered.findIndex(o => o.value === value)
      setHighlightIdx(idx >= 0 ? idx : 0)
    }
  }, [open])

  // 滚动到高亮项
  useEffect(() => {
    if (!open || highlightIdx < 0) return
    const list = listRef.current
    if (!list) return
    const item = list.children[highlightIdx] as HTMLElement
    if (item) item.scrollIntoView({ block: 'nearest' })
  }, [highlightIdx, open])

  const handleSelect = useCallback((val: string) => {
    onChange(val)
    setOpen(false)
    setSearch('')
  }, [onChange])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!open) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
        e.preventDefault()
        setOpen(true)
      }
      return
    }
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault()
        setHighlightIdx(prev => Math.min(prev + 1, filtered.length - 1))
        break
      case 'ArrowUp':
        e.preventDefault()
        setHighlightIdx(prev => Math.max(prev - 1, 0))
        break
      case 'Enter':
        e.preventDefault()
        if (highlightIdx >= 0 && highlightIdx < filtered.length) {
          handleSelect(filtered[highlightIdx].value)
        }
        break
      case 'Escape':
        setOpen(false)
        setSearch('')
        break
    }
  }

  return (
    <div ref={containerRef} style={{ position: 'relative', ...style }} onKeyDown={handleKeyDown}>
      {/* 触发按钮 */}
      <button
        type="button"
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
        style={{
          width: '100%', padding: '10px 14px', paddingRight: 36,
          border: open ? '1px solid var(--accent)' : '1px solid var(--border-subtle)',
          borderRadius: 10, fontSize: 14, cursor: disabled ? 'not-allowed' : 'pointer',
          backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
          textAlign: 'left', display: 'flex', alignItems: 'center', gap: 8,
          boxShadow: open ? '0 0 0 3px rgba(16, 185, 129, 0.15)' : 'none',
          transition: 'border-color 0.15s, box-shadow 0.15s',
          opacity: disabled ? 0.5 : 1,
        }}
      >
        <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {selectedOption ? selectedOption.label : <span style={{ color: 'var(--text-muted)' }}>{placeholder}</span>}
        </span>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ flexShrink: 0, transition: 'transform 0.2s', transform: open ? 'rotate(180deg)' : 'rotate(0deg)' }}>
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>

      {/* 下拉面板 */}
      {open && (
        <div style={{
          position: 'absolute', top: 'calc(100% + 4px)', left: 0, right: 0,
          backgroundColor: 'var(--bg-elevated)', border: '1px solid var(--border-subtle)',
          borderRadius: 10, boxShadow: '0 12px 40px rgba(0,0,0,0.4)',
          zIndex: 50, overflow: 'hidden',
          animation: 'selectFadeIn 0.15s ease',
        }}>
          {/* 搜索框 */}
          {searchable && (
            <div style={{ padding: '8px 8px 4px' }}>
              <input
                ref={searchRef}
                value={search}
                onChange={e => { setSearch(e.target.value); setHighlightIdx(0) }}
                placeholder="Search..."
                style={{
                  width: '100%', padding: '8px 10px', border: '1px solid var(--border-subtle)',
                  borderRadius: 6, fontSize: 13, boxSizing: 'border-box',
                  backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
                  outline: 'none',
                }}
              />
            </div>
          )}

          {/* 选项列表 */}
          <div ref={listRef} style={{ maxHeight: 240, overflowY: 'auto', padding: 4 }}>
            {filtered.length === 0 ? (
              <div style={{ padding: '12px 14px', fontSize: 13, color: 'var(--text-muted)', textAlign: 'center' }}>
                No results
              </div>
            ) : (
              filtered.map((opt, idx) => {
                const isSelected = opt.value === value
                const isHighlighted = idx === highlightIdx
                return (
                  <div
                    key={opt.value}
                    onClick={() => handleSelect(opt.value)}
                    onMouseEnter={() => setHighlightIdx(idx)}
                    style={{
                      padding: '8px 12px', borderRadius: 6, cursor: 'pointer',
                      fontSize: 13, display: 'flex', alignItems: 'center', gap: 8,
                      backgroundColor: isHighlighted ? 'var(--accent-bg)' : 'transparent',
                      color: isSelected ? 'var(--accent)' : 'var(--text-primary)',
                      fontWeight: isSelected ? 600 : 400,
                      transition: 'background-color 0.1s',
                    }}
                  >
                    {/* 选中标记 */}
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none"
                      stroke={isSelected ? 'var(--accent)' : 'transparent'} strokeWidth="2"
                      strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                    <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {opt.label}
                    </span>
                  </div>
                )
              })
            )}
          </div>
        </div>
      )}

      <style>{`
        @keyframes selectFadeIn {
          from { opacity: 0; transform: translateY(-4px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  )
}
