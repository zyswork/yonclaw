/**
 * ChatTab - 对话 Tab 组件
 *
 * 从 AgentDetailPage.tsx 提取的独立组件
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import mermaid from 'mermaid'
import katex from 'katex'
import 'katex/dist/katex.min.css'
import { useI18n } from '../i18n'
import { toast, friendlyError } from '../hooks/useToast'
import { useConfirm, showConfirm } from '../hooks/useConfirm'
import { useVoiceInput } from '../hooks/useVoiceInput'
import { useVoiceOutput } from '../hooks/useVoiceOutput'
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts'
import Select from './Select'

// 初始化 Mermaid
mermaid.initialize({
  startOnLoad: false,
  theme: 'dark',
  securityLevel: 'loose',
})

// 自定义 renderer：代码块增加语言标签和复制按钮，mermaid 代码块用占位符
const codeBlockRenderer: import('marked').MarkedExtension = {
  renderer: {
    code({ text, lang }: { text: string; lang?: string }) {
      // Mermaid 代码块：输出占位标记，后续由 React 组件替换
      if (lang === 'mermaid') {
        const encoded = btoa(unescape(encodeURIComponent(text)))
        return `<div class="mermaid-placeholder" data-mermaid-code="${encoded}"></div>`
      }
      const escaped = text
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
      const langLabel = lang ? `<span class="code-lang">${lang}</span>` : ''
      const copyBtn = `<button class="code-copy-btn">Copy</button>`
      return `<div class="code-block-wrapper"><div class="code-block-header">${langLabel}${copyBtn}</div><pre><code class="language-${lang || 'text'}">${escaped}</code></pre></div>`
    },
  },
}

// KaTeX marked 扩展：块级公式 $$...$$ 和行内公式 $...$
const katexExtension: import('marked').MarkedExtension = {
  extensions: [{
    name: 'math-block',
    level: 'block' as const,
    start(src: string) { return src.indexOf('$$') },
    tokenizer(src: string) {
      const match = src.match(/^\$\$([\s\S]+?)\$\$/)
      if (match) {
        return { type: 'math-block', raw: match[0], text: match[1].trim() }
      }
      return undefined
    },
    renderer(token) {
      const text = (token as unknown as { text: string }).text
      try {
        return `<div class="katex-display-wrapper">${katex.renderToString(text, { displayMode: true, throwOnError: false })}</div>`
      } catch {
        return `<pre class="katex-error">${text}</pre>`
      }
    }
  }, {
    name: 'math-inline',
    level: 'inline' as const,
    start(src: string) { return src.indexOf('$') },
    tokenizer(src: string) {
      // 不匹配 $$ （那是块级），不匹配换行，不匹配纯数字如 $100
      const match = src.match(/^\$([^\$\n]+?)\$/)
      if (match && !/^\d+$/.test(match[1].trim())) {
        return { type: 'math-inline', raw: match[0], text: match[1].trim() }
      }
      return undefined
    },
    renderer(token) {
      const text = (token as unknown as { text: string }).text
      try {
        return katex.renderToString(text, { displayMode: false, throwOnError: false })
      } catch {
        return `<code class="katex-error">${text}</code>`
      }
    }
  }]
}

marked.use({ breaks: true, gfm: true, ...codeBlockRenderer })
marked.use(katexExtension)

// ─── Types ───────────────────────────────────────────────────

interface Agent {
  id: string
  name: string
  model: string
  systemPrompt: string
  temperature: number | null
  maxTokens: number | null
  createdAt: number
  updatedAt: number
}

interface Session {
  id: string
  agentId: string
  title: string
  createdAt: number
  lastMessageAt: number | null
}

interface Message {
  role: 'user' | 'assistant' | 'tool' | 'system'
  content: string
  toolName?: string
  thinking?: string
  isError?: boolean
  seq?: number
}

interface Skill {
  name: string
  description: string
  enabled: boolean
  path: string
}

interface SubagentInfo {
  id: string
  name: string
  status: string
}

interface ProviderInfo {
  id: string
  name: string
  apiType: string
  enabled: boolean
  apiKey?: string
  baseUrl?: string
  models: Array<{ id: string; name?: string }>
}

interface SubagentRecord {
  id: string
  parentId: string
  name: string
  task: string
  status: string
  result: string | null
  createdAt: number
  finishedAt: number | null
  timeoutSecs: number
}

// ─── Mermaid 渲染组件 ───────────────────────────────────────

function MermaidBlock({ code }: { code: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [svg, setSvg] = useState('')
  const [error, setError] = useState('')

  useEffect(() => {
    const id = `mermaid-${Math.random().toString(36).slice(2, 10)}`
    mermaid.render(id, code).then(({ svg: renderedSvg }) => {
      setSvg(renderedSvg)
    }).catch((e) => {
      setError(String(e))
    })
  }, [code])

  if (error) {
    return (
      <pre style={{
        color: 'var(--error)', background: 'var(--error-bg)',
        padding: '12px 16px', borderRadius: 8, fontSize: 12, margin: '8px 0',
        whiteSpace: 'pre-wrap', wordBreak: 'break-word',
      }}>
        Mermaid Error: {error}
      </pre>
    )
  }
  if (!svg) {
    return (
      <div style={{
        padding: '16px', margin: '8px 0', borderRadius: 8,
        background: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
        color: 'var(--text-muted)', textAlign: 'center', fontSize: 13,
      }}>
        Loading diagram...
      </div>
    )
  }
  const cleanSvg = DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } })
  return (
    <div
      ref={containerRef}
      className="mermaid-rendered"
      dangerouslySetInnerHTML={{ __html: cleanSvg }}
    />
  )
}

// ─── Helper Functions & Components ──────────────────────────

/** Markdown 渲染：支持 Mermaid 流程图和 KaTeX 数学公式 */
function renderMd(text: string) {
  const html = marked.parse(text, { async: false }) as string
  const clean = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [
      'a','b','blockquote','br','button','code','del','div','em',
      'h1','h2','h3','h4','hr','i','li','ol','p','pre','span',
      'strong','table','tbody','td','th','thead','tr','ul','img',
      // KaTeX 生成的标签
      'math','semantics','mrow','mi','mo','mn','msup','msub','mfrac',
      'mover','munder','munderover','msqrt','mroot','mtable','mtr','mtd',
      'mtext','mspace','annotation','menclose','mpadded','mphantom',
      // SVG 标签（Mermaid 占位符的 class/data 属性需要）
      'svg','path','circle','line','rect','text','g','defs','marker',
      'polygon','polyline','foreignObject','ellipse','tspan',
    ],
    ALLOWED_ATTR: [
      'class','href','rel','target','title','src','alt','start','style',
      // Mermaid 占位符
      'data-mermaid-code',
      // SVG 属性
      'd','fill','stroke','viewBox','xmlns','transform',
      'cx','cy','r','x','y','width','height',
      'text-anchor','dominant-baseline','font-size','font-family',
      'marker-end','points','x1','y1','x2','y2',
      'stroke-width','stroke-dasharray','stroke-linecap','stroke-linejoin',
      'opacity','rx','ry','dx','dy','id',
      // KaTeX 属性
      'mathvariant','encoding','accent','accentunder','columnalign',
      'columnspacing','rowspacing','columnlines','rowlines','frame',
    ],
  })

  // 检测多媒体内容（音频/图片路径）
  const mediaElements = extractMediaFromText(text)

  // 检查是否包含 mermaid 占位符
  if (!clean.includes('mermaid-placeholder')) {
    return (
      <div>
        <div className="markdown-body" dangerouslySetInnerHTML={{ __html: clean }} />
        {mediaElements}
      </div>
    )
  }

  // 包含 mermaid 占位符：拆分 HTML，在占位符位置插入 MermaidBlock 组件
  const parts: React.ReactNode[] = []
  const regex = /<div class="mermaid-placeholder" data-mermaid-code="([^"]+)"><\/div>/g
  let lastIndex = 0
  let match: RegExpExecArray | null

  while ((match = regex.exec(clean)) !== null) {
    // 占位符前的 HTML
    if (match.index > lastIndex) {
      const htmlBefore = clean.slice(lastIndex, match.index)
      parts.push(<span key={`html-${lastIndex}`} dangerouslySetInnerHTML={{ __html: htmlBefore }} />)
    }
    // 解码 mermaid 代码并渲染
    try {
      const code = decodeURIComponent(escape(atob(match[1])))
      parts.push(<MermaidBlock key={`mermaid-${match.index}`} code={code} />)
    } catch {
      parts.push(<pre key={`mermaid-err-${match.index}`} style={{ color: 'var(--error)' }}>Failed to decode mermaid block</pre>)
    }
    lastIndex = match.index + match[0].length
  }

  // 剩余 HTML
  if (lastIndex < clean.length) {
    parts.push(<span key={`html-${lastIndex}`} dangerouslySetInnerHTML={{ __html: clean.slice(lastIndex) }} />)
  }

  return (
    <div>
      <div className="markdown-body">{parts}</div>
      {mediaElements}
    </div>
  )
}

/** 从文本中检测媒体标记和文件路径，返回内嵌播放器/图片/下载按钮 */
function extractMediaFromText(text: string): React.ReactNode[] {
  const elements: React.ReactNode[] = []
  const handledPaths = new Set<string>()

  // 优先解析结构化 media_tag: <!--media:{"type":"audio","path":"...","mime":"..."}-->
  const mediaTagRegex = /<!--media:(.*?)-->/g
  let match
  while ((match = mediaTagRegex.exec(text)) !== null) {
    try {
      const media = JSON.parse(match[1])
      const { type, path, mime, data } = media
      if (!path) continue
      handledPaths.add(path)

      if (type === 'audio') {
        elements.push(<AudioPlayer key={`media-audio-${path}`} filePath={path} />)
      } else if (type === 'image') {
        elements.push(
          <div key={`media-img-${path}`} style={{ marginTop: 8 }}>
            <ImagePreview filePath={path} />
          </div>
        )
      } else if (type === 'video') {
        elements.push(
          <div key={`media-video-${path}`} style={{ marginTop: 8, padding: '8px 12px', borderRadius: 8, background: 'var(--bg-glass)', border: '1px solid var(--border-subtle)', fontSize: 13 }}>
            🎬 视频: {path.split('/').pop()} <a href="#" onClick={async (e) => { e.preventDefault(); try { const { open } = await import('@tauri-apps/api/shell'); await open(path) } catch {} }} style={{ color: 'var(--accent)' }}>打开播放</a>
          </div>
        )
      } else {
        // file / document
        elements.push(
          <div key={`media-file-${path}`} style={{ marginTop: 8, padding: '8px 12px', borderRadius: 8, background: 'var(--bg-glass)', border: '1px solid var(--border-subtle)', fontSize: 13 }}>
            📄 {path.split('/').pop()} <a href="#" onClick={async (e) => { e.preventDefault(); try { const { open } = await import('@tauri-apps/api/shell'); await open(path) } catch {} }} style={{ color: 'var(--accent)' }}>打开</a>
          </div>
        )
      }
    } catch {}
  }

  // 解析执行进度标记: <!--exec:{"tool":"xxx","index":1,"total":3,"status":"running"}-->
  const execTagRegex = /<!--exec:(.*?)-->/g
  const execSteps: { tool: string; index: number; total: number; status: string }[] = []
  let execMatch
  while ((execMatch = execTagRegex.exec(text)) !== null) {
    try {
      const data = JSON.parse(execMatch[1])
      // 去重（同一个 tool+index 只保留最后一个状态）
      const existing = execSteps.findIndex(s => s.tool === data.tool && s.index === data.index)
      if (existing >= 0) execSteps[existing] = data
      else execSteps.push(data)
    } catch {}
  }
  if (execSteps.length > 0) {
    elements.push(
      <div key="exec-progress" style={{
        marginTop: 6, padding: '6px 10px', borderRadius: 8,
        background: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
        fontSize: 12, color: 'var(--text-muted)', display: 'flex', gap: 6, flexWrap: 'wrap',
      }}>
        {execSteps.map((s, i) => (
          <span key={i} style={{ display: 'inline-flex', alignItems: 'center', gap: 3 }}>
            {s.status === 'running' ? '⚙️' : '✅'} {s.tool}
          </span>
        ))}
      </div>
    )
  }

  // fallback: 检测音频文件路径（兼容旧消息，不重复）
  const audioRegex = /(\/[^\s]+\.(mp3|aiff|wav|ogg|m4a))/gi
  const audioMatches = text.match(audioRegex)
  if (audioMatches) {
    audioMatches.forEach((path, i) => {
      if (handledPaths.has(path.trim())) return
      handledPaths.add(path.trim())
      elements.push(<AudioPlayer key={`audio-${i}`} filePath={path.trim()} />)
    })
  }

  // 检测远程图片 URL（非 markdown 格式的裸 URL）
  const imgUrlRegex = /(https?:\/\/[^\s]+\.(png|jpg|jpeg|gif|webp|svg)(\?[^\s]*)?)/gi
  const imgMatches = text.match(imgUrlRegex)
  if (imgMatches) {
    // 只渲染不在 markdown ![](url) 中的裸 URL
    const mdImgRegex = /!\[[^\]]*\]\([^)]+\)/g
    const mdImgs = text.match(mdImgRegex)?.map(m => {
      const urlMatch = m.match(/\(([^)]+)\)/)
      return urlMatch?.[1] || ''
    }) || []

    const seen = new Set<string>()
    imgMatches.forEach((url, i) => {
      if (seen.has(url) || mdImgs.includes(url)) return
      seen.add(url)
      elements.push(
        <img key={`img-${i}`} src={url} alt=""
          style={{ maxWidth: '100%', maxHeight: 400, borderRadius: 8, marginTop: 8, display: 'block' }}
          onError={(e) => { (e.target as HTMLImageElement).style.display = 'none' }}
        />
      )
    })
  }

  return elements
}
/** 思考中动画 */
function ThinkingIndicator() {
  const { t } = useI18n()
  const [dots, setDots] = useState('')
  const [elapsed, setElapsed] = useState(0)

  useEffect(() => {
    const dotTimer = setInterval(() => setDots(d => d.length >= 3 ? '' : d + '.'), 500)
    const elapsedTimer = setInterval(() => setElapsed(e => e + 1), 1000)
    return () => { clearInterval(dotTimer); clearInterval(elapsedTimer) }
  }, [])

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--text-muted)', fontSize: 13 }}>
      <span style={{
        display: 'inline-block', width: 8, height: 8, borderRadius: '50%',
        backgroundColor: 'var(--accent)', animation: 'pulse 1.5s ease-in-out infinite',
      }} />
      <span>{t('agentDetail.thinking')}{dots}</span>
      {elapsed > 3 && <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{elapsed}s</span>}
      <style>{`@keyframes pulse { 0%, 100% { opacity: 0.3; transform: scale(0.8); } 50% { opacity: 1; transform: scale(1.2); } }`}</style>
    </div>
  )
}
function SessionItem({ s, activeSession, onSelect, onDelete, onExport, renamingSession, renameValue, setRenameValue, onStartRename, onFinishRename, onCancelRename, isSystem }: {
  s: Session; activeSession: string; onSelect: () => void; onDelete: () => void; onExport?: () => void
  renamingSession: string; renameValue: string; setRenameValue: (v: string) => void
  onStartRename: () => void; onFinishRename: (v: string) => void; onCancelRename: () => void
  isSystem?: boolean
}) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center',
      padding: '8px 12px', cursor: 'pointer', fontSize: isSystem ? 12 : 13,
      backgroundColor: s.id === activeSession ? 'var(--accent-bg)' : 'transparent',
      borderBottom: '1px solid var(--border-subtle)',
      borderLeft: s.id === activeSession ? '3px solid var(--accent)' : '3px solid transparent',
      opacity: isSystem ? 0.7 : 1,
    }}>
      {renamingSession === s.id ? (
        <input autoFocus value={renameValue} onChange={(e) => setRenameValue(e.target.value)}
          onBlur={() => onFinishRename(renameValue)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) onFinishRename(renameValue); if (e.key === 'Escape') onCancelRename() }}
          style={{ flex: 1, padding: '2px 4px', border: '1px solid var(--accent)', borderRadius: 3, fontSize: 13, outline: 'none' }}
        />
      ) : (
        <div onClick={onSelect} onDoubleClick={onStartRename}
          style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
          title={isSystem ? s.title : useI18n.getState().t('agentDetail.hintRename')}
        >
          {isSystem && <span style={{ color: 'var(--text-muted)', marginRight: 4 }}>&#x23F0;</span>}
          {s.title}
        </div>
      )}
      {onExport && <button onClick={(e) => { e.stopPropagation(); onExport() }}
        style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', padding: '2px', flexShrink: 0, display: 'flex', alignItems: 'center' }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--accent)' }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--text-muted)' }}
        title="Export"
      ><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg></button>}
      <button onClick={(e) => { e.stopPropagation(); onDelete() }}
        style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', padding: '2px', flexShrink: 0, display: 'flex', alignItems: 'center' }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.color = '#ef4444' }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.color = 'var(--text-muted)' }}
        title={useI18n.getState().t('agentDetailSub.deleteTitle')}
      ><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>
    </div>
  )
}

/** 转义正则特殊字符 */
function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** 搜索高亮：将匹配的关键词用 <mark> 包裹 */
function highlightText(text: string, query: string) {
  if (!query) return text
  const parts = text.split(new RegExp(`(${escapeRegExp(query)})`, 'gi'))
  return parts.map((part, i) =>
    part.toLowerCase() === query.toLowerCase()
      ? <mark key={i} style={{ backgroundColor: 'var(--warning, #f59e0b)', color: '#000', borderRadius: 2, padding: '0 2px' }}>{part}</mark>
      : part
  )
}

/** 用户消息内容渲染（支持 ![图片](/path) 显示缩略图） */
function UserMessageContent({ content, searchQuery }: { content: string; searchQuery?: string }) {
  // 检测 Markdown 图片引用: ![图片](/path/to/file.jpg)
  const imgRegex = /!\[([^\]]*)\]\(([^)]+)\)/g
  const parts: Array<{ type: 'text' | 'image'; value: string }> = []
  let lastIdx = 0
  let match
  while ((match = imgRegex.exec(content)) !== null) {
    if (match.index > lastIdx) {
      const text = content.slice(lastIdx, match.index).trim()
      if (text) parts.push({ type: 'text', value: text })
    }
    parts.push({ type: 'image', value: match[2] })
    lastIdx = match.index + match[0].length
  }
  if (lastIdx < content.length) {
    const text = content.slice(lastIdx).trim()
    if (text) parts.push({ type: 'text', value: text })
  }

  if (parts.length === 0 || !parts.some(p => p.type === 'image')) {
    return <>{searchQuery ? highlightText(content, searchQuery) : content}</>
  }

  return (
    <div>
      {parts.map((p, i) =>
        p.type === 'image' ? (
          <img
            key={i}
            src={convertLocalPath(p.value)}
            alt="image"
            style={{ maxWidth: '100%', maxHeight: 300, borderRadius: 8, marginTop: 4, display: 'block' }}
            onError={(e) => { (e.target as HTMLImageElement).style.display = 'none' }}
          />
        ) : (
          <span key={i}>{searchQuery ? highlightText(p.value, searchQuery) : p.value}</span>
        )
      )}
    </div>
  )
}

/** 把本地文件路径转为 Tauri asset URL */
/** 音频播放组件 — 通过 Tauri 读取本地文件，用 blob URL 播放 */
function AudioPlayer({ filePath }: { filePath: string }) {
  const audioRef = useRef<HTMLAudioElement>(null)
  const [blobUrl, setBlobUrl] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  const loadAudio = async () => {
    if (blobUrl) return // 已加载
    setLoading(true)
    setError('')
    try {
      // 通过 Rust 后端读取文件为 base64（绕过 Tauri FS scope 限制）
      const b64 = await invoke<string>('read_file_base64', { path: filePath })
      const ext = filePath.split('.').pop()?.toLowerCase() || ''
      const mimeMap: Record<string, string> = {
        mp3: 'audio/mpeg', wav: 'audio/wav', m4a: 'audio/mp4',
        ogg: 'audio/ogg', aiff: 'audio/aiff', aac: 'audio/aac',
      }
      const mime = mimeMap[ext] || 'audio/mpeg'
      // base64 → binary → blob
      const binary = atob(b64)
      const bytes = new Uint8Array(binary.length)
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
      const blob = new Blob([bytes], { type: mime })
      const url = URL.createObjectURL(blob)
      setBlobUrl(url)
      setTimeout(() => audioRef.current?.play(), 100)
    } catch (e) {
      setError(String(e))
    }
    setLoading(false)
  }

  useEffect(() => { return () => { if (blobUrl) URL.revokeObjectURL(blobUrl) } }, [blobUrl])

  return (
    <div style={{
      marginTop: 8, padding: '10px 14px', borderRadius: 10,
      backgroundColor: 'var(--bg-glass)', border: '1px solid var(--border-subtle)',
      display: 'flex', alignItems: 'center', gap: 10,
    }}>
      <span style={{ fontSize: 13, color: 'var(--text-muted)' }}>Audio</span>
      {blobUrl ? (
        <audio ref={audioRef} controls style={{ flex: 1, height: 36 }} src={blobUrl} />
      ) : (
        <button onClick={loadAudio} disabled={loading} style={{
          flex: 1, padding: '8px 16px', borderRadius: 6, border: '1px solid var(--accent)',
          background: 'transparent', color: 'var(--accent)', cursor: 'pointer', fontSize: 13,
        }}>
          {loading ? '加载中...' : error ? '播放失败，点击重试' : '点击播放'}
        </button>
      )}
      <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
        {filePath.split('/').pop()}
      </span>
    </div>
  )
}

/** 图片预览组件 — 通过 Tauri 读取本地图片 */
function ImagePreview({ filePath }: { filePath: string }) {
  const [src, setSrc] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      setLoading(true)
      try {
        const b64 = await invoke<string>('read_file_base64', { path: filePath })
        if (cancelled) return
        const ext = filePath.split('.').pop()?.toLowerCase() || 'png'
        const mimeMap: Record<string, string> = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml' }
        setSrc(`data:${mimeMap[ext] || 'image/png'};base64,${b64}`)
      } catch {}
      setLoading(false)
    })()
    return () => { cancelled = true }
  }, [filePath])

  if (loading) return <span style={{ fontSize: 12, color: 'var(--text-muted)' }}>加载图片...</span>
  if (!src) return null
  return (
    <img src={src} alt={filePath.split('/').pop() || ''} style={{ maxWidth: '100%', maxHeight: 400, borderRadius: 8, cursor: 'pointer' }}
      onClick={async () => { try { const { open } = await import('@tauri-apps/api/shell'); await open(filePath) } catch {} }}
    />
  )
}

function convertLocalPath(path: string): string {
  // Tauri 1.x asset protocol
  // 正确格式: https://asset.localhost/{encoded_path}
  // 路径需要 encodeURIComponent 但保留结构
  if (path.startsWith('/') || path.startsWith('~') || /^[A-Z]:[/\\]/i.test(path)) {
    // window.__TAURI__ 提供的 convertFileSrc
    if ((window as any).__TAURI__?.tauri?.convertFileSrc) {
      return (window as any).__TAURI__.tauri.convertFileSrc(path)
    }
    // fallback: 手动构造（Tauri 1.x 格式）
    return `https://asset.localhost/${encodeURIComponent(path)}`
  }
  return path
}

/** 功能栏：消息计数 + 压缩按钮（带 loading 和结果提示） */
function ToolBar({ messageCount, showCompact, onCompact, agentId, sessionId, streaming, hud }: {
  messageCount: number; showCompact: boolean; onCompact: () => Promise<string>
  agentId?: string; sessionId?: string
  streaming?: boolean; hud?: { round: number; tokens: number; cost: number; lastTool: string; lastToolOk: boolean }
}) {
  const { t } = useI18n()
  const [status, setStatus] = useState<'idle' | 'loading' | 'done' | 'error'>('idle')
  const [msg, setMsg] = useState('')
  const [ctx, setCtx] = useState<{ total: number; max_context: number; usage_percent: string; system_prompt: number; messages: number; tools: number; soul_files: number } | null>(null)
  const [ctxVersion, setCtxVersion] = useState(0)

  // 切换 session 时重置状态
  useEffect(() => {
    setStatus('idle')
    setMsg('')
    setCtx(null)
  }, [sessionId])

  // 加载上下文使用情况
  const refreshCtx = useCallback(() => {
    if (agentId && sessionId) {
      invoke<any>('get_context_usage', { agentId, sessionId }).then((data) => {
        setCtx(data)
      }).catch((e) => {
        console.error('[ToolBar] get_context_usage error:', e)
      })
    }
  }, [agentId, sessionId])

  useEffect(() => {
    refreshCtx()
  }, [refreshCtx, messageCount, ctxVersion])

  const handleCompact = async () => {
    setStatus('loading'); setMsg('')
    try {
      const result = await onCompact()
      setStatus('done'); setMsg('Done')
      // 压缩完成后直接刷新上下文数据
      refreshCtx()
      setTimeout(() => { refreshCtx() }, 1000)
      setTimeout(() => { refreshCtx() }, 3000)
      setTimeout(() => { setStatus('idle'); setMsg('') }, 2000)
    } catch (e) {
      console.error('[ToolBar] handleCompact error:', e)
      setStatus('error'); setMsg(String(e).slice(0, 80))
      setTimeout(() => { setStatus('idle'); setMsg('') }, 3000)
    }
  }

  const pct = ctx ? parseFloat(ctx.usage_percent) : 0
  const barColor = pct > 80 ? 'var(--error)' : pct > 50 ? '#f0ad4e' : 'var(--accent)'

  return (
    <div style={{ padding: '4px 16px', borderTop: '1px solid var(--border-subtle)', fontSize: 11, color: 'var(--text-muted)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span>{messageCount}{t('agentDetail.messages')}</span>
        {streaming && hud && hud.round > 0 && (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: 'var(--text-muted)' }}>
            | R{hud.round}
            {hud.tokens > 0 && <span>{(hud.tokens / 1000).toFixed(1)}K tok</span>}
            {hud.cost > 0.001 && <span style={{ color: hud.cost > 0.5 ? 'var(--error)' : undefined }}>${hud.cost.toFixed(3)}</span>}
            {hud.lastTool && <span style={{ color: hud.lastToolOk ? 'var(--accent)' : 'var(--error)' }}>{hud.lastTool} {hud.lastToolOk ? '✓' : '✗'}</span>}
          </span>
        )}
        {ctx && (
          <span title={`System: ${ctx.system_prompt} | Soul: ${ctx.soul_files} | Messages: ${ctx.messages} | Tools: ${ctx.tools}`}
            style={{ cursor: 'help' }}>
            | Context: {(ctx.total / 1000).toFixed(1)}K / {(ctx.max_context / 1000).toFixed(0)}K ({ctx.usage_percent}%)
          </span>
        )}
        {msg && <span style={{ color: status === 'error' ? '#ef4444' : '#22c55e' }}>{msg}</span>}
        <span style={{ flex: 1 }} />
        {showCompact && (
          <button onClick={handleCompact} disabled={status === 'loading'}
            style={{ background: 'none', border: '1px solid var(--border-subtle)', borderRadius: 4, padding: '2px 8px', fontSize: 11, cursor: status === 'loading' ? 'wait' : 'pointer', color: status === 'loading' ? 'var(--border-subtle)' : 'var(--text-secondary)' }}>
            {status === 'loading' ? t('agentDetail.compacting') : t('agentDetail.compactHistory')}
          </button>
        )}
      </div>
      {/* 上下文使用条 */}
      {ctx && pct > 0 && (
        <div style={{ marginTop: 3, height: 3, borderRadius: 2, backgroundColor: 'var(--border-subtle)', overflow: 'hidden' }}>
          <div style={{ height: '100%', width: `${Math.min(pct, 100)}%`, backgroundColor: barColor, borderRadius: 2, transition: 'width 0.3s' }} />
        </div>
      )}
    </div>
  )
}

/** 格式化工具调用内容（尝试 JSON 美化） */
function formatToolContent(content: string): string {
  try {
    const parsed = JSON.parse(content)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return content
  }
}

/** 从工具内容中提取执行时间（如果有的话） */
function extractToolMeta(content: string): { duration?: string; success: boolean } {
  // 尝试从内容中判断是否成功（包含 error/Error 关键字视为失败）
  const hasError = /\berror\b|"error"|Error:|failed|exception/i.test(content)
  // 尝试从内容中提取耗时信息
  const durationMatch = content.match(/(\d+(?:\.\d+)?)\s*(?:ms|milliseconds|seconds|s)\b/)
  return {
    duration: durationMatch ? durationMatch[0] : undefined,
    success: !hasError,
  }
}

/** 工具调用卡片的齿轮图标 */
function ToolGearIcon({ size = 14, color = 'currentColor' }: { size?: number; color?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3"/>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
    </svg>
  )
}

// ─── 消息分组：连续工具调用合并 ──────────────────────────
interface MessageGroup {
  type: 'single' | 'tool-group'
  messages: Message[]
  startIdx: number
}

function groupMessages(msgs: Message[]): MessageGroup[] {
  const groups: MessageGroup[] = []
  let i = 0
  while (i < msgs.length) {
    if (msgs[i].role === 'tool') {
      const toolMsgs: Message[] = []
      const start = i
      while (i < msgs.length && msgs[i].role === 'tool') {
        toolMsgs.push(msgs[i])
        i++
      }
      if (toolMsgs.length >= 2) {
        groups.push({ type: 'tool-group', messages: toolMsgs, startIdx: start })
      } else {
        groups.push({ type: 'single', messages: toolMsgs, startIdx: start })
      }
    } else {
      groups.push({ type: 'single', messages: [msgs[i]], startIdx: i })
      i++
    }
  }
  return groups
}

// ─── 推理过程折叠组件 ──────────────────────────────────────
function ThinkingBlock({ thinking }: { thinking: string }) {
  const [expanded, setExpanded] = useState(false)
  const { t } = useI18n()
  return (
    <div style={{
      marginBottom: 8, marginLeft: 0, maxWidth: 560,
      borderRadius: 10, overflow: 'hidden',
      border: '1px solid rgba(139,92,246,0.2)',
      backgroundColor: 'rgba(139,92,246,0.04)',
    }}>
      <div
        onClick={() => setExpanded(e => !e)}
        style={{
          padding: '6px 12px',
          display: 'flex', alignItems: 'center', gap: 8,
          cursor: 'pointer', userSelect: 'none',
        }}
      >
        <span style={{ fontSize: 14 }}>&#x1F4AD;</span>
        <span style={{ fontSize: 12, color: 'var(--text-muted)', fontWeight: 500 }}>
          {t('agentDetail.thinking') || 'Thinking'}
        </span>
        <div style={{ flex: 1 }} />
        <svg
          width="14" height="14" viewBox="0 0 24 24" fill="none"
          stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ transition: 'transform 0.2s ease', transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
        >
          <polyline points="6 9 12 15 18 9"/>
        </svg>
      </div>
      {expanded && (
        <div style={{
          padding: '8px 12px', borderTop: '1px solid rgba(139,92,246,0.15)',
        }}>
          <pre style={{
            margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-word',
            fontFamily: "'SF Mono', Monaco, monospace", fontSize: 12, lineHeight: 1.5,
            color: 'var(--text-secondary)',
            backgroundColor: 'rgba(0,0,0,0.1)', borderRadius: 6,
            padding: '8px 10px', maxHeight: 300, overflow: 'auto',
          }}>
            {thinking}
          </pre>
        </div>
      )}
    </div>
  )
}

/** 打字指示器 - 三点跳动 */
function TypingIndicator() {
  const [dots, setDots] = useState('')
  useEffect(() => {
    const timer = setInterval(() => setDots(d => d.length >= 3 ? '' : d + '.'), 400)
    return () => clearInterval(timer)
  }, [])
  return (
    <div style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '8px 14px', borderRadius: 12,
      backgroundColor: 'var(--bg-elevated, #f1f5f9)',
      border: '1px solid var(--border-subtle, #e2e8f0)',
      fontSize: 13, color: 'var(--text-muted, #64748b)',
    }}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none"
        stroke="var(--accent, #10b981)" strokeWidth="2" strokeLinecap="round"
        style={{ animation: 'spin 1s linear infinite' }}>
        <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
      </svg>
      <span style={{ fontWeight: 500, minWidth: 42 }}>思考中{dots}</span>
    </div>
  )
}

// ─── 工具组合并卡片组件 ──────────────────────────────────────
function ToolGroupCard({ messages, groupKey, expandedTools, toggleTool }: {
  messages: Message[]
  groupKey: string
  expandedTools: Set<string>
  toggleTool: (key: string) => void
}) {
  const { t } = useI18n()
  const isGroupExpanded = expandedTools.has(groupKey)
  const metas = messages.map(m => extractToolMeta(m.content || ''))
  const successCount = metas.filter(m => m.success).length
  const failCount = metas.length - successCount
  const allSuccess = failCount === 0
  const accentColor = allSuccess ? 'var(--accent, #34d399)' : 'var(--error, #ef4444)'
  const statusBg = allSuccess ? 'rgba(52,211,153,0.06)' : 'rgba(239,68,68,0.06)'

  return (
    <div style={{
      marginBottom: 6, marginLeft: 38, maxWidth: 560,
      borderRadius: 10, overflow: 'hidden',
      border: '1px solid var(--border-subtle)',
      borderLeft: `3px solid ${accentColor}`,
      backgroundColor: 'var(--bg-elevated)',
      transition: 'all 0.2s ease',
    }}>
      {/* 组头部 */}
      <div
        onClick={() => toggleTool(groupKey)}
        style={{
          padding: '8px 12px',
          display: 'flex', alignItems: 'center', gap: 8,
          cursor: 'pointer', userSelect: 'none',
          backgroundColor: statusBg,
        }}
      >
        <ToolGearIcon size={14} color={accentColor} />
        <strong style={{ fontSize: 12, color: 'var(--text-primary)', fontFamily: "'SF Mono', Monaco, monospace" }}>
          {messages.length} {t('common.tools') || 'tools'}
        </strong>
        <div style={{ flex: 1 }} />
        <span style={{
          display: 'inline-flex', alignItems: 'center', gap: 4,
          fontSize: 11, color: accentColor, fontWeight: 500,
        }}>
          {allSuccess ? (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={accentColor} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="20 6 9 17 4 12"/>
            </svg>
          ) : (
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={accentColor} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          )}
          {allSuccess
            ? (t('common.success') || 'Success')
            : `${successCount} ${t('common.success') || 'ok'} / ${failCount} ${t('common.failed') || 'fail'}`
          }
        </span>
        <svg
          width="14" height="14" viewBox="0 0 24 24" fill="none"
          stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
          style={{ transition: 'transform 0.2s ease', transform: isGroupExpanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
        >
          <polyline points="6 9 12 15 18 9"/>
        </svg>
      </div>
      {/* 收起状态：工具名列表 */}
      {!isGroupExpanded && (
        <div style={{ padding: '4px 12px 6px' }}>
          {messages.map((m, idx) => {
            const meta = metas[idx]
            const c = meta.success ? 'var(--accent, #34d399)' : 'var(--error, #ef4444)'
            return (
              <div key={idx} style={{
                display: 'inline-flex', alignItems: 'center', gap: 4,
                marginRight: 10, fontSize: 11, color: 'var(--text-muted)',
                fontFamily: "'SF Mono', Monaco, monospace",
              }}>
                {meta.success ? (
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke={c} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="20 6 9 17 4 12"/>
                  </svg>
                ) : (
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke={c} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                  </svg>
                )}
                <span>{m.toolName || t('common.tools')}</span>
                {meta.duration && <span style={{ color: 'var(--text-muted)', opacity: 0.6 }}>{meta.duration}</span>}
              </div>
            )
          })}
        </div>
      )}
      {/* 展开状态：每个工具详情 */}
      {isGroupExpanded && messages.map((m, idx) => {
        const meta = metas[idx]
        const itemKey = `${groupKey}-${idx}`
        const itemExpanded = expandedTools.has(itemKey)
        const c = meta.success ? 'var(--accent, #34d399)' : 'var(--error, #ef4444)'
        return (
          <div key={idx} style={{ borderTop: '1px solid var(--border-subtle)' }}>
            <div
              onClick={() => toggleTool(itemKey)}
              style={{
                padding: '6px 12px',
                display: 'flex', alignItems: 'center', gap: 8,
                cursor: 'pointer', userSelect: 'none',
              }}
            >
              {meta.success ? (
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke={c} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12"/>
                </svg>
              ) : (
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke={c} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                </svg>
              )}
              <span style={{ fontSize: 12, fontFamily: "'SF Mono', Monaco, monospace", fontWeight: 500, color: 'var(--text-primary)' }}>
                {m.toolName || t('common.tools')}
              </span>
              <div style={{ flex: 1 }} />
              {meta.duration && <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{meta.duration}</span>}
              <svg
                width="12" height="12" viewBox="0 0 24 24" fill="none"
                stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                style={{ transition: 'transform 0.2s ease', transform: itemExpanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
              >
                <polyline points="6 9 12 15 18 9"/>
              </svg>
            </div>
            {itemExpanded && m.content && (
              <div style={{ padding: '4px 12px 8px' }}>
                <pre style={{
                  margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                  fontFamily: "'SF Mono', Monaco, monospace", fontSize: 11, lineHeight: 1.5,
                  color: 'var(--text-secondary)',
                  backgroundColor: 'rgba(0,0,0,0.15)', borderRadius: 6,
                  padding: '8px 10px', maxHeight: 280, overflow: 'auto',
                }}>
                  {formatToolContent(m.content)}
                </pre>
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

const isSystemSession = (title: string) =>
  title.startsWith('cron-') || title.startsWith('[cron]') ||
  title.startsWith('heartbeat-') || title.startsWith('[heartbeat]') ||
  title.startsWith('[group]')

const actionBtnStyle: React.CSSProperties = {
  background: 'none', border: '1px solid var(--border-subtle)', borderRadius: 6,
  cursor: 'pointer', padding: '4px 6px', lineHeight: 1,
  color: 'var(--text-muted)', display: 'flex', alignItems: 'center',
  transition: 'all 0.15s',
}


export default function ChatTab({ agentId }: { agentId: string }) {
  const { t } = useI18n()
  const confirm = useConfirm()
  const [sessions, setSessions] = useState<Session[]>([])
  const [activeSession, setActiveSession] = useState('')
  const [messages, setMessages] = useState<Message[]>([])
  const [displayCount, setDisplayCount] = useState(50)
  const [input, setInput] = useState('')
  const [streaming, setStreaming] = useState(false)
  // 中文输入法组合状态跟踪（nativeEvent.isComposing 在 Tauri WebView 中不可靠）
  const composingRef = useRef(false)
  // HUD 实时状态
  const [hud, setHud] = useState({ round: 0, tokens: 0, cost: 0, lastTool: '', lastToolOk: true })

  // Python 沙箱状态检查（首次挂载时检查一次）
  useEffect(() => {
    let cancelled = false
    const check = async () => {
      try {
        const status = await invoke<{ status: string }>('get_python_sandbox_status')
        if (cancelled) return
        if (status.status === 'initializing') {
          toast.info('Python 环境准备中，首次使用需要约 10 秒...')
          // 轮询直到就绪
          const poll = setInterval(async () => {
            try {
              const s = await invoke<{ status: string }>('get_python_sandbox_status')
              if (s.status === 'ready') {
                clearInterval(poll)
                toast.success('Python 环境已就绪')
              }
            } catch {}
          }, 5000)
          setTimeout(() => clearInterval(poll), 120000) // 2 分钟后停止轮询
        }
      } catch {}
    }
    check()
    return () => { cancelled = true }
  }, [])

  // 切换 Agent 时重置状态，避免旧 Agent 的对话内容残留
  const prevAgentIdRef = useRef(agentId)
  useEffect(() => {
    if (prevAgentIdRef.current !== agentId) {
      prevAgentIdRef.current = agentId
      setActiveSession('')
      setMessages([])
      setSessions([])
      setStreaming(false)
    }
  }, [agentId])
  // 对话模式：flash=快速回复(不使用工具), standard=标准, thinking=深度思考
  const [chatMode, setChatMode] = useState<'flash' | 'standard' | 'thinking'>('standard')
  const streamingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastTokenTimeRef = useRef<number>(0)
  const staleCheckRef = useRef<ReturnType<typeof setInterval> | null>(null)
  // 安全超时：streaming 超过 120 秒自动恢复
  useEffect(() => {
    if (streaming) {
      streamingTimerRef.current = setTimeout(() => setStreaming(false), 120_000)
      lastTokenTimeRef.current = Date.now()
      // 30 秒无 token 心跳检测
      staleCheckRef.current = setInterval(() => {
        if (lastTokenTimeRef.current && Date.now() - lastTokenTimeRef.current > 30_000) {
          toast.info('AI 正在思考中，请稍候...')
          lastTokenTimeRef.current = Date.now() // 重置，避免重复提示
        }
      }, 10_000)
    } else if (streamingTimerRef.current) {
      clearTimeout(streamingTimerRef.current)
      streamingTimerRef.current = null
      if (staleCheckRef.current) { clearInterval(staleCheckRef.current); staleCheckRef.current = null }
    }
    return () => {
      if (streamingTimerRef.current) clearTimeout(streamingTimerRef.current)
      if (staleCheckRef.current) { clearInterval(staleCheckRef.current); staleCheckRef.current = null }
    }
  }, [streaming])
  const [renamingSession, setRenamingSession] = useState('')
  const [renameValue, setRenameValue] = useState('')
  const [pendingImages, setPendingImages] = useState<string[]>([]) // base64 data URLs
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [showSystemSessions, setShowSystemSessions] = useState(false)
  const [selectedSessions, setSelectedSessions] = useState<Set<string>>(new Set())
  const [selectMode, setSelectMode] = useState(false)
  const [sessionSearch, setSessionSearch] = useState('')
  // 对话内搜索状态
  const [chatSearchOpen, setChatSearchOpen] = useState(false)
  const [chatSearchQuery, setChatSearchQuery] = useState('')
  const [chatSearchIdx, setChatSearchIdx] = useState(0)
  const chatSearchInputRef = useRef<HTMLInputElement>(null)
  const messagesContainerRef = useRef<HTMLDivElement>(null)

  const [expandedTools, setExpandedTools] = useState<Set<string>>(new Set())
  const toggleTool = (key: string) => setExpandedTools(prev => {
    const next = new Set(prev)
    next.has(key) ? next.delete(key) : next.add(key)
    return next
  })
  const messagesEndRef = useRef<HTMLDivElement>(null)

  // 对话内搜索：计算匹配消息索引
  const chatSearchMatches = useMemo(() => {
    if (!chatSearchQuery.trim()) return [] as number[]
    const q = chatSearchQuery.toLowerCase()
    const matches: number[] = []
    messages.forEach((m, i) => {
      if (m.content && typeof m.content === 'string' && m.content.toLowerCase().includes(q)) {
        matches.push(i)
      }
    })
    return matches
  }, [chatSearchQuery, messages])

  // 搜索：跳转到匹配的消息位置
  useEffect(() => {
    if (chatSearchMatches.length === 0 || !messagesContainerRef.current) return
    const idx = chatSearchMatches[chatSearchIdx] ?? chatSearchMatches[0]
    const el = messagesContainerRef.current.querySelector(`[data-msg-idx="${idx}"]`)
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }, [chatSearchIdx, chatSearchMatches])

  // 代码块复制按钮 — 事件委托
  const handleCodeCopyClick = useCallback((e: React.MouseEvent) => {
    const target = e.target as HTMLElement
    if (target.classList.contains('code-copy-btn')) {
      e.preventDefault()
      const wrapper = target.closest('.code-block-wrapper')
      const code = wrapper?.querySelector('code')?.textContent || ''
      navigator.clipboard.writeText(code).then(() => {
        target.textContent = 'Copied'
        setTimeout(() => { target.textContent = 'Copy' }, 1500)
      }).catch(() => {
        // 回退：选中文本
        const range = document.createRange()
        const codeEl = wrapper?.querySelector('code')
        if (codeEl) {
          range.selectNodeContents(codeEl)
          const sel = window.getSelection()
          sel?.removeAllRanges()
          sel?.addRange(range)
        }
      })
    }
  }, [])
  // 编辑消息状态
  const [editingIdx, setEditingIdx] = useState<number | null>(null)
  const [editingContent, setEditingContent] = useState('')

  // 语音输入/输出
  const { isRecording, isTranscribing, startRecording, stopRecording, cancelRecording, error: voiceError } = useVoiceInput()
  const { isSpeaking, speak, stop: stopSpeaking, voiceEnabled, setVoiceEnabled } = useVoiceOutput()
  const voiceEnabledRef = useRef(voiceEnabled)
  useEffect(() => { voiceEnabledRef.current = voiceEnabled }, [voiceEnabled])
  const speakRef = useRef(speak)
  useEffect(() => { speakRef.current = speak }, [speak])
  const [recordingDuration, setRecordingDuration] = useState(0)
  const recordingTimerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // 录音计时器
  useEffect(() => {
    if (isRecording) {
      setRecordingDuration(0)
      recordingTimerRef.current = setInterval(() => setRecordingDuration(d => d + 1), 1000)
    } else {
      if (recordingTimerRef.current) { clearInterval(recordingTimerRef.current); recordingTimerRef.current = null }
      setRecordingDuration(0)
    }
    return () => { if (recordingTimerRef.current) clearInterval(recordingTimerRef.current) }
  }, [isRecording])

  // 语音错误提示
  useEffect(() => { if (voiceError) toast.error(voiceError) }, [voiceError])

  // 多 Agent 面板状态
  const [showAgentPanel, setShowAgentPanel] = useState(false)
  const [otherAgents, setOtherAgents] = useState<Agent[]>([])
  const [activeSubagents, setActiveSubagents] = useState<SubagentRecord[]>([])
  const [agentMsgTarget, setAgentMsgTarget] = useState('')
  const [agentMsgContent, setAgentMsgContent] = useState('')
  const [a2aTarget, setA2aTarget] = useState('')
  const [a2aTopic, setA2aTopic] = useState('')
  const [mailboxMsgs, setMailboxMsgs] = useState<any[]>([])

  // 加载其他 Agent 列表和子 Agent
  const loadAgentPanel = useCallback(async () => {
    try {
      const [agents, subs] = await Promise.all([
        invoke<Agent[]>('list_agents'),
        invoke<SubagentRecord[]>('list_subagents', { agentId }),
      ])
      setOtherAgents(agents.filter(a => a.id !== agentId))
      setActiveSubagents(subs || [])
    } catch (e) { console.error('loadAgentPanel:', e) }
    // 加载邮箱
    try {
      const msgs = await invoke<any[]>('get_agent_mailbox', { agentId })
      if (msgs.length > 0) setMailboxMsgs(msgs)
    } catch (e) { console.error('loadMailbox failed:', e) }
  }, [agentId])

  useEffect(() => {
    if (showAgentPanel) loadAgentPanel()
  }, [showAgentPanel, loadAgentPanel])

  // 定期刷新子 Agent 状态（兜底轮询，每 10s）
  useEffect(() => {
    if (!showAgentPanel) return
    const timer = setInterval(loadAgentPanel, 10000)
    return () => clearInterval(timer)
  }, [showAgentPanel, loadAgentPanel])

  // 事件驱动：子代理派发/完成时刷新面板 + 完成结果注入对话
  useEffect(() => {
    let cancelled = false
    let unlistenFn: (() => void) | null = null

    listen<any>('agent-event', (e) => {
      if (cancelled) return
      const type = e.payload?.type

      // HUD 状态追踪
      if (type === 'llm_start') {
        setHud(h => ({ ...h, round: (e.payload.round || 0) }))
      } else if (type === 'llm_done') {
        const inp = e.payload.input_tokens || 0
        const out = e.payload.output_tokens || 0
        setHud(h => ({ ...h, tokens: h.tokens + inp + out, cost: h.cost + (inp * 0.000003 + out * 0.000015) }))
      } else if (type === 'tool_done') {
        setHud(h => ({ ...h, lastTool: e.payload.tool_name || '', lastToolOk: e.payload.success !== false }))
      }

      // 刷新子 Agent 面板
      if (showAgentPanel && (type === 'subagent_spawned' || type === 'subagent_complete')) {
        loadAgentPanel()
      }
      // 异步子代理完成：将结果作为 assistant 消息追加到对话中
      if (type === 'subagent_complete' && e.payload?.parent_agent_id === agentId) {
        const { success_count, fail_count, summary } = e.payload
        const status = fail_count > 0
          ? `${success_count} ${t('common.success') || 'succeeded'}, ${fail_count} ${t('common.failed') || 'failed'}`
          : `${success_count} ${t('agentDetailSub.tasksComplete') || 'tasks completed'}`
        setMessages(prev => [...prev, {
          role: 'assistant',
          content: `**[${t('multiAgent.subagentResult') || 'Subagent Result'}]** ${status}\n\n${summary || ''}`,
        }])
        // 同时从 DB 刷新完整消息（subagent 可能写了 DB 消息）
        loadMessagesRef.current()
      }
    }).then(f => {
      if (cancelled) {
        // 组件已卸载或 effect 已重新执行，立即清理
        f()
      } else {
        unlistenFn = f
      }
    })

    return () => {
      cancelled = true
      if (unlistenFn) unlistenFn()
    }
  }, [showAgentPanel, loadAgentPanel, agentId])

  // 发送 Agent 间消息
  const handleSendAgentMsg = async () => {
    if (!agentMsgTarget || !agentMsgContent.trim()) return
    try {
      await invoke('send_agent_message', { fromId: agentId, toId: agentMsgTarget, content: agentMsgContent.trim() })
      toast.success(t('agentDetailSub.messageSent'))
      setAgentMsgContent('')
    } catch (e) { toast.error(friendlyError(e)) }
  }

  // 发起 A2A 对话（通过在当前对话中发送指令消息）
  const handleA2aChat = async () => {
    if (!a2aTarget || !a2aTopic.trim() || !activeSession) return
    const targetAgent = otherAgents.find(a => a.id === a2aTarget)
    const msg = `请与 ${targetAgent?.name || a2aTarget.slice(0, 8)} 进行多轮对话讨论以下话题：${a2aTopic.trim()}`
    setInput(msg)
    setA2aTopic('')
    setShowAgentPanel(false)
  }

  // 邀请 Agent 协作（通过 collaborate 工具）
  const handleInviteAgent = async (targetId: string) => {
    if (!activeSession) return
    const targetAgent = otherAgents.find(a => a.id === targetId)
    const msg = `请邀请 ${targetAgent?.name || targetId.slice(0, 8)} 加入当前对话协作`
    setInput(msg)
    setShowAgentPanel(false)
  }

  // 图片处理：文件→压缩后 base64（最大 1200px，JPEG 质量 0.7）
  const addImageFiles = (files: FileList | File[]) => {
    Array.from(files).forEach(file => {
      if (!file.type.startsWith('image/')) return
      const reader = new FileReader()
      reader.onload = () => {
        const img = new Image()
        img.onload = () => {
          const MAX_DIM = 1200
          let w = img.width, h = img.height
          if (w > MAX_DIM || h > MAX_DIM) {
            const scale = MAX_DIM / Math.max(w, h)
            w = Math.round(w * scale)
            h = Math.round(h * scale)
          }
          const canvas = document.createElement('canvas')
          canvas.width = w; canvas.height = h
          const ctx = canvas.getContext('2d')!
          ctx.drawImage(img, 0, 0, w, h)
          const dataUrl = canvas.toDataURL('image/jpeg', 0.7)
          setPendingImages(prev => [...prev, dataUrl])

          // 持久化到磁盘
          try {
            const parts = dataUrl.split(',')
            const base64 = parts.length > 1 ? parts[1] : null
            if (base64) {
              invoke('save_chat_image', { agentId, base64Data: base64 })
                .catch((e) => console.warn('Image save failed:', e))
            }
          } catch (e) { console.warn('Image processing error:', e) }
        }
        img.src = reader.result as string
      }
      reader.readAsDataURL(file)
    })
  }

  // 粘贴处理
  const handlePaste = (e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items
    if (!items) return
    const imageFiles: File[] = []
    for (let i = 0; i < items.length; i++) {
      if (items[i].type.startsWith('image/')) {
        const file = items[i].getAsFile()
        if (file) imageFiles.push(file)
      }
    }
    if (imageFiles.length > 0) {
      e.preventDefault()
      addImageFiles(imageFiles)
    }
  }

  // 拖拽处理
  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (e.dataTransfer.files.length > 0) {
      const files = Array.from(e.dataTransfer.files)
      const images = files.filter(f => f.type.startsWith('image/'))
      const others = files.filter(f => !f.type.startsWith('image/'))
      // 图片：添加到图片附件
      if (images.length > 0) addImageFiles(images)
      // 非图片文件：将文件路径插入输入框，让 agent 用 doc_parse/file_read 处理
      if (others.length > 0) {
        const paths = others.map(f => (f as any).path || f.name).join('\n')
        setInput(prev => prev ? `${prev}\n${paths}` : paths)
        toast.info(`已添加 ${others.length} 个文件路径`)
      }
    }
  }
  const streamBuf = useRef('')
  const thinkingBuf = useRef('')
  const streamingSessionRef = useRef('')
  const sendingRef = useRef(false)

  // 序列计数器：防止快速切换会话/session 时旧请求覆盖新数据
  const sessionLoadSeqRef = useRef(0)
  const messageLoadSeqRef = useRef(0)

  const loadSessions = useCallback(async () => {
    const seq = ++sessionLoadSeqRef.current
    try {
      const result = await invoke<Session[]>('list_sessions', { agentId })
      if (sessionLoadSeqRef.current !== seq) return // 过期响应，丢弃
      setSessions(result)
      if (result.length > 0 && !activeSession) {
        setActiveSession(result[0].id)
      }
    } catch (e) {
      if (sessionLoadSeqRef.current !== seq) return
      console.error(e)
    }
  }, [agentId, activeSession])

  useEffect(() => { loadSessions() }, [loadSessions])

  // 快捷键系统（使用全局 hook）
  useKeyboardShortcuts({
    // Cmd+N: 新建会话
    'cmd+n': () => createSession(),
    // Cmd+/: 聚焦搜索
    'cmd+/': () => {
      setSessionSearch(prev => prev ? '' : ' ') // 切换搜索框
      setTimeout(() => setSessionSearch(''), 10) // 清空触发 focus
    },
    // Cmd+E: 导出当前会话
    'cmd+e': () => { if (activeSession) exportSession(activeSession) },
    // Cmd+F: 对话内搜索
    'cmd+f': () => {
      if (activeSession) {
        setChatSearchOpen(true)
        setTimeout(() => chatSearchInputRef.current?.focus(), 50)
      }
    },
    // Escape: 关闭搜索/取消选择
    'escape': () => {
      if (chatSearchOpen) { setChatSearchOpen(false); setChatSearchQuery(''); setChatSearchIdx(0) }
      else if (sessionSearch) setSessionSearch('')
      else if (selectMode) { setSelectMode(false); setSelectedSessions(new Set()) }
    },
  })

  const loadMessages = useCallback(async () => {
    if (!activeSession) return
    const seq = ++messageLoadSeqRef.current
    try {
      const structured = await invoke<any[]>('load_structured_messages', { sessionId: activeSession, limit: displayCount })
      if (messageLoadSeqRef.current !== seq) return // 过期响应，丢弃
      if (structured && structured.length > 0) {
        const parsed: Message[] = []
        for (const m of structured) {
          if (m.role === 'system') continue
          if (m.role === 'tool') {
            parsed.push({ role: 'tool', content: m.content || '', toolName: m.name || t('common.tools'), seq: m.seq })
          } else if (m.role === 'assistant' && m.tool_calls) {
            // 先展示工具卡片，再展示后续 assistant 消息中的最终回复
            // 跳过本消息的前置文本（preamble），避免工具在下、文字在上的顺序问题
            for (const tc of (Array.isArray(m.tool_calls) ? m.tool_calls : [])) {
              parsed.push({ role: 'tool', content: '', toolName: tc.function?.name || tc.name || t('common.tools'), seq: m.seq })
            }
          } else {
            // 过滤 Anthropic 格式的 tool_result 消息（不显示原始 JSON）
            const contentStr = typeof m.content === 'string' ? m.content : JSON.stringify(m.content || '')
            if (contentStr.includes('"tool_result"') || contentStr.includes('"tool_use"')) continue
            parsed.push({ role: m.role, content: contentStr, seq: m.seq })
          }
        }
        setMessages(parsed)
      } else {
        const msgs = await invoke<Message[]>('get_session_messages', { agentId, sessionId: activeSession })
        if (messageLoadSeqRef.current !== seq) return // 二次检查（fallback 路径）
        setMessages(msgs)
      }
    } catch (e) {
      if (messageLoadSeqRef.current !== seq) return
      console.error(e)
    }
  }, [agentId, activeSession, displayCount])

  useEffect(() => { loadMessages() }, [loadMessages])

  // 切换会话时关闭搜索
  useEffect(() => {
    setChatSearchOpen(false)
    setChatSearchQuery('')
    setChatSearchIdx(0)
  }, [activeSession])

  // 定时检查当前会话是否有新消息（兼容 Telegram/外部消息）
  useEffect(() => {
    if (!activeSession || streaming) return
    const msgCountRef = { current: messages.length }
    const interval = setInterval(async () => {
      try {
        const structured = await invoke<any[]>('load_structured_messages', { sessionId: activeSession, limit: 50 })
        const newCount = structured?.length || 0
        if (newCount !== msgCountRef.current) {
          msgCountRef.current = newCount
          loadMessagesRef.current()
        }
      } catch { /* ignore */ }
    }, 3000) // 每 3 秒检查一次
    return () => clearInterval(interval)
  }, [activeSession, streaming])

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // 用 ref 跟踪最新的 activeSession（避免闭包陷阱）
  const activeSessionRef = useRef(activeSession)
  useEffect(() => { activeSessionRef.current = activeSession }, [activeSession])
  const loadSessionsRef = useRef(loadSessions)
  useEffect(() => { loadSessionsRef.current = loadSessions }, [loadSessions])
  const loadMessagesRef = useRef(loadMessages)
  useEffect(() => { loadMessagesRef.current = loadMessages }, [loadMessages])

  // 统一事件监听（参考 OpenClaw：事件直接携带消息内容，带 sessionId 过滤）
  useEffect(() => {
    // 桌面对话的流式 token（携带 sessionId，过滤非当前会话）
    const unlisten1 = listen<any>('llm-token', (e) => {
      lastTokenTimeRef.current = Date.now() // 心跳更新
      // 解析 payload：新格式 {sessionId, text}，兼容旧格式 string
      const payload = e.payload
      const tokenSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      const tokenText = typeof payload === 'object' ? (payload?.text ?? '') : String(payload ?? '')

      // 重试重置信号：清除当前流式内容
      if (tokenText === '\x00__XIANZHU_RETRY__') {
        streamBuf.current = ''
        thinkingBuf.current = ''
        setMessages((prev) => {
          const copy = [...prev]
          if (copy.length > 0 && copy[copy.length - 1].role === 'assistant') {
            copy[copy.length - 1] = { ...copy[copy.length - 1], content: '', thinking: '' }
          }
          return copy
        })
        return
      }

      // 跨会话过滤：如果事件携带 sessionId，必须匹配当前活跃会话
      if (tokenSessionId && tokenSessionId !== activeSessionRef.current) return
      // 兼容旧逻辑：无 sessionId 时使用 streamingSessionRef 过滤
      if (!tokenSessionId && streamingSessionRef.current && streamingSessionRef.current !== activeSessionRef.current) return

      const THINKING_PREFIX = '\x01THINKING\x01'
      if (tokenText.startsWith(THINKING_PREFIX)) {
        // Thinking delta：追加到 thinking buffer
        thinkingBuf.current += tokenText.slice(THINKING_PREFIX.length)
        setMessages((prev) => {
          const copy = [...prev]
          if (copy.length > 0 && copy[copy.length - 1].role === 'assistant') {
            copy[copy.length - 1] = { ...copy[copy.length - 1], thinking: thinkingBuf.current }
          }
          return copy
        })
      } else {
        streamBuf.current += tokenText
        setMessages((prev) => {
          const copy = [...prev]
          if (copy.length > 0 && copy[copy.length - 1].role === 'assistant') {
            copy[copy.length - 1] = { ...copy[copy.length - 1], content: streamBuf.current }
          }
          return copy
        })
      }
    })
    const unlisten2 = listen<any>('llm-done', (e) => {
      // 跨会话过滤：新格式携带 sessionId
      const payload = e.payload
      const doneSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      if (doneSessionId && doneSessionId !== activeSessionRef.current) return
      if (!doneSessionId && streamingSessionRef.current && streamingSessionRef.current !== activeSessionRef.current) return
      streamingSessionRef.current = ''
      setStreaming(false)
      streamBuf.current = ''
      thinkingBuf.current = ''
      // 桌面通知：窗口不活跃时通知用户 AI 已回复
      if (document.hidden) {
        try {
          import('@tauri-apps/api/notification').then(({ isPermissionGranted, requestPermission, sendNotification }) => {
            isPermissionGranted().then(granted => {
              if (!granted) { requestPermission(); return }
              sendNotification({ title: '衔烛 Claw', body: 'AI 回复已完成' })
            })
          })
        } catch {}
      }
      // 语音朗读：自动朗读最后一条 AI 回复
      if (voiceEnabledRef.current) {
        setMessages(prev => {
          const last = prev.length > 0 ? prev[prev.length - 1] : null
          if (last && last.role === 'assistant' && last.content) {
            speakRef.current(typeof last.content === 'string' ? last.content : String(last.content))
          }
          return prev // 不修改 messages
        })
      }
      // 从 DB 重新加载结构化消息（消除 streaming 临时状态与 DB 数据的差异，避免闪烁）
      loadMessagesRef.current()
      // 立即刷新会话列表（同步命名已完成）；延迟再刷一次（LLM 异步命名约 1-3 秒后完成）
      loadSessionsRef.current()
      setTimeout(() => loadSessionsRef.current(), 3000)
    })
    const unlisten3 = listen<any>('llm-error', (e) => {
      // 跨会话过滤
      const payload = e.payload
      const errSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      const errText = typeof payload === 'object' ? (payload?.text ?? String(payload)) : String(payload ?? '')
      if (errSessionId && errSessionId !== activeSessionRef.current) return
      if (!errSessionId && streamingSessionRef.current && streamingSessionRef.current !== activeSessionRef.current) return
      streamingSessionRef.current = ''
      setStreaming(false)
      streamBuf.current = ''
      thinkingBuf.current = ''
      setMessages((prev) => [...prev, { role: 'system', content: errText, isError: true }])
    })

    // 外部消息事件（Telegram/Mobile）— 带 sessionId 和消息内容
    const unlisten4 = listen<any>('chat-event', (e) => {
      const { type, sessionId, role, content, source } = e.payload || {}

      // 始终刷新会话列表（新消息可能创建了新 session）
      if (type === 'message' || type === 'done') {
        loadSessionsRef.current()
      }

      // 只处理当前正在查看的 session
      if (sessionId !== activeSessionRef.current) return

      switch (type) {
        case 'message':
          // 外部用户消息直接追加（不读 DB）
          setMessages((prev) => [...prev, { role: role || 'user', content: content || '' }])
          break

        case 'thinking':
          // 追加空的 assistant 消息（显示思考动画）
          setMessages((prev) => [...prev, { role: 'assistant', content: '' }])
          setStreaming(true)
          break

        case 'token':
          // 流式更新最后一条 assistant 消息
          setMessages((prev) => {
            const copy = [...prev]
            if (copy.length > 0 && copy[copy.length - 1].role === 'assistant') {
              copy[copy.length - 1] = { ...copy[copy.length - 1], content: content || '' }
            }
            return copy
          })
          break

        case 'done':
          // 完成：更新最后一条为完整回复
          setStreaming(false)
          setMessages((prev) => {
            const copy = [...prev]
            if (copy.length > 0 && copy[copy.length - 1].role === 'assistant') {
              copy[copy.length - 1] = { ...copy[copy.length - 1], content: content || '' }
            } else {
              copy.push({ role: 'assistant', content: content || '' })
            }
            return copy
          })
          break
      }
    })

    return () => {
      unlisten1.then((f) => f())
      unlisten2.then((f) => f())
      unlisten3.then((f) => f())
      unlisten4.then((f) => f())
    }
  }, [])

  const createSession = async () => {
    try {
      const session = await invoke<Session>('create_session', {
        agentId,
        title: t('agentDetailSub.conversationN', { n: sessions.length + 1 }),
      })
      setSessions((prev) => [session, ...prev])
      setActiveSession(session.id)
      setMessages([])
    } catch (e) { toast.error(friendlyError(e)) }
  }

  const renameSession = async (sessionId: string, newTitle: string) => {
    if (!newTitle.trim()) { setRenamingSession(''); return }
    try {
      await invoke('rename_session', { sessionId, title: newTitle.trim() })
      setSessions((prev) => prev.map((s) => s.id === sessionId ? { ...s, title: newTitle.trim() } : s))
    } catch (e) { toast.error(friendlyError(e)) }
    setRenamingSession('')
  }

  const deleteSession = async (sessionId: string) => {
    if (!await confirm(t('agentDetail.confirmDeleteSession'))) return
    try {
      await invoke('delete_session', { sessionId })
      setSessions((prev) => prev.filter((s) => s.id !== sessionId))
      if (activeSession === sessionId) {
        setActiveSession('')
        setMessages([])
      }
    } catch (e) { toast.error(friendlyError(e)) }
  }

  const batchDeleteSessions = async () => {
    if (selectedSessions.size === 0) return
    if (!await confirm(t('agentDetail.confirmBatchDelete', { count: selectedSessions.size }))) return
    try {
      let deleted = 0
      const failed: string[] = []
      for (const sid of selectedSessions) {
        try {
          await invoke('delete_session', { sessionId: sid })
          deleted++
        } catch (e) { console.error('deleteSession failed:', e); failed.push(sid) }
      }
      if (failed.length > 0) {
        toast.error(`${failed.length} session(s) failed to delete`)
      }
      setSessions(prev => prev.filter(s => !selectedSessions.has(s.id) || failed.includes(s.id)))
      if (selectedSessions.has(activeSession)) {
        setActiveSession('')
        setMessages([])
      }
      setSelectedSessions(new Set())
      setSelectMode(false)
      toast.success(t('agentDetail.batchDeleteDone', { count: selectedSessions.size }))
    } catch (e) { toast.error(friendlyError(e)) }
  }

  const exportSession = async (sessionId: string, _format: 'markdown' | 'json' = 'markdown') => {
    try {
      // 加载结构化消息（含 tool_calls、tool_result）
      const structured = await invoke<any[]>('load_structured_messages', { sessionId, limit: 500 })
      if (!structured || structured.length === 0) {
        toast.error(t('agentDetail.noMessagesExport'))
        return
      }

      // 构建 Markdown 内容
      const session = sessions.find(s => s.id === sessionId)
      const title = session?.title || 'Conversation'
      const date = new Date().toISOString().split('T')[0]

      let md = `# ${title}\n\n`
      md += `> Exported from XianZhu on ${date}\n\n---\n\n`

      for (const m of structured) {
        if (m.role === 'system') continue
        if (m.role === 'user') {
          md += `## 👤 User\n\n${m.content || ''}\n\n`
        } else if (m.role === 'assistant') {
          md += `## 🤖 Assistant\n\n${m.content || ''}\n\n`
          if (m.tool_calls) {
            for (const tc of m.tool_calls) {
              md += `> 🔧 Tool: ${tc.function?.name || tc.name}\n\n`
            }
          }
        } else if (m.role === 'tool') {
          const toolContent = (m.content || '').slice(0, 200)
          md += `> 🔧 Tool Result (${m.name || 'tool'}):\n> ${toolContent}${(m.content || '').length > 200 ? '...' : ''}\n\n`
        }
        md += '---\n\n'
      }

      const safeTitle = title.replace(/[^\w\u4e00-\u9fff]/g, '_')
      const fileName = `${safeTitle}_${date}.md`

      // 优先使用 Tauri save dialog，失败时 fallback 到浏览器 Blob 下载
      let saved = false
      try {
        const { save } = await import('@tauri-apps/api/dialog')
        const { writeTextFile } = await import('@tauri-apps/api/fs')
        const filePath = await save({
          defaultPath: fileName,
          filters: [{ name: 'Markdown', extensions: ['md'] }]
        })
        if (filePath) {
          await writeTextFile(filePath, md)
          saved = true
        } else {
          // 用户取消了保存对话框
          return
        }
      } catch {
        // Tauri dialog/fs 不可用，fallback 到浏览器下载
        const blob = new Blob([md], { type: 'text/markdown' })
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = fileName
        a.click()
        URL.revokeObjectURL(url)
        saved = true
      }

      if (saved) {
        toast.success(t('common.exportSuccess') || t('agentDetailSub.exportedAs', { title }) || 'Exported')
      }
    } catch (e) {
      console.error('Export error:', e)
      toast.error(t('common.exportFailed') || String(e))
    }
  }

  const toggleSessionSelect = (sid: string) => {
    setSelectedSessions(prev => {
      const next = new Set(prev)
      if (next.has(sid)) next.delete(sid); else next.add(sid)
      return next
    })
  }

  // ─── 斜杠命令处理 ─────────────────────────────
  const handleSlashCommand = async (cmd: string, args: string): Promise<string | null> => {
    switch (cmd) {
      case 'help':
        return t('agentDetailSub.slashHelp')

      case 'new':
        await createSession()
        return t('agentDetail.successNewSession')

      case 'clear':
        if (activeSession) {
          await invoke('clear_history', { sessionId: activeSession })
          setMessages([])
          return t('agentDetail.successCleared')
        }
        return t('agentDetail.errorNoSession')

      case 'compact':
        if (activeSession) {
          toast.info(t('agentDetail.compacting'))
          invoke<string>('compact_session', { agentId, sessionId: activeSession })
            .then(r => {
              loadMessages()
              toast.success(t('agentDetail.compactDone') || 'Compacted')
            })
            .catch(e => {
              toast.error(`${t('common.error')}: ${e}`)
            })
          return ''
        }
        return t('agentDetail.errorNoSession')

      case 'rename': {
        if (!args.trim()) return t('chatPage.renameUsage')
        if (activeSession) {
          await invoke('rename_session', { sessionId: activeSession, title: args.trim() })
          setSessions((prev) => prev.map((s) => s.id === activeSession ? { ...s, title: args.trim() } : s))
          return t('agentDetail.successRenamed', { name: args.trim() })
        }
        return t('agentDetail.errorNoSession')
      }

      case 'model': {
        if (!args.trim()) {
          try {
            const detail = await invoke<Record<string, any>>('get_agent_detail', { agentId })
            return `${t('agentDetail.currentModel')}: **${detail?.model}**\nTemperature: ${detail?.temperature ?? 'default'}\nMax Tokens: ${detail?.maxTokens ?? 'default'}`
          } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
        }
        try {
          await invoke('update_agent', { agentId, model: args.trim() })
          return `${t('agentDetail.switchedTo')} **${args.trim()}**`
        } catch (e) { return t('agentDetailSub.switchFailed') + ': ' + e }
      }

      case 'temp': {
        const tempVal = parseFloat(args)
        if (isNaN(tempVal) || tempVal < 0 || tempVal > 2) return '/temp <0-2>, e.g. /temp 0.7'
        try {
          await invoke('update_agent', { agentId, temperature: tempVal })
          return `${t('agentDetail.tempAdjusted')} **${tempVal}**`
        } catch (e) { return 'Failed: ' + e }
      }

      case 'status': {
        try {
          const h = await invoke<any>('health_check')
          return `## ${t('agentDetailSub.systemStatus')}\n- ${t('agentDetailSub.statusLabel')}: ${h.status}\n- ${t('agentDetailSub.agentCount')}: ${h.agents}\n- ${t('agentDetailSub.memoryCount')}: ${h.memories}\n- ${t('agentDetailSub.todayToken')}: ${h.today_tokens?.toLocaleString()}\n- ${t('agentDetailSub.responseCacheCount')}: ${h.response_cache_entries}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'usage': {
        try {
          const stats = await invoke<any>('get_token_stats', { agentId, days: 7 })
          const total = stats?.totalTokens || 0
          const cost = stats?.estimatedCost || 0
          return `## ${t('agentDetailSub.tokenUsage')}\n- ${t('agentDetailSub.totalTokens')}: ${total.toLocaleString()} tokens\n- ${t('agentDetailSub.inputTokens')}: ${(stats?.inputTokens || 0).toLocaleString()}\n- ${t('agentDetailSub.outputTokens')}: ${(stats?.outputTokens || 0).toLocaleString()}\n- ${t('agentDetailSub.estimatedCost')}: $${cost.toFixed(4)}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'tools': {
        try {
          const detail = await invoke<Record<string, any>>('get_agent_detail', { agentId })
          return `## ${t('agentDetailSub.availableTools')} (${detail?.toolCount || 0})\n\n${t('agentDetailSub.toolsDesc')}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'skills': {
        try {
          const list = await invoke<Skill[]>('list_skills', { agentId })
          if (!list?.length) return t('agentDetailSub.noInstalledSkills')
          return `## ${t('agentDetailSub.installedSkills')} (${list.length})\n\n${list.map((s: Skill) => `- **${s.name}** ${s.enabled ? '✓' : '✗'} ${s.description || ''}`).join('\n')}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'providers': {
        try {
          const providers = await invoke<ProviderInfo[]>('get_providers')
          return `## ${t('agentDetailSub.providerConfig')} (${providers?.length || 0})\n\n${(providers || []).map((p: ProviderInfo) => {
            const hasKey = p.apiKey && p.apiKey.length > 0
            const models = (p.models || []).map((m: { id: string; name?: string }) => m.name || m.id).join(', ')
            return `- **${p.name}** (${p.apiType}) ${p.enabled ? '✓' : '✗'} Key:${hasKey ? t('agentDetailSub.keyYes') : t('agentDetailSub.keyNo')}\n  ${t('agentDetailSub.modelLabel')}: ${models || t('agentDetailSub.keyNo')}\n  ${t('agentDetailSub.urlLabel')}: ${p.baseUrl || t('agentDetailSub.defaultLabel')}`
          }).join('\n')}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'memory': {
        try {
          const detail = await invoke<Record<string, any>>('get_agent_detail', { agentId })
          return `## ${t('agentDetailSub.memoryStats')}\n- ${t('agentDetailSub.memoryCount')}: ${detail?.memories?.length || 0}\n- ${t('agentDetailSub.vectorCount')}: ${detail?.vectorCount || 0}\n- ${t('agentDetailSub.embeddingCacheCount')}: ${detail?.embeddingCacheCount || 0}\n- ${t('agentDetailSub.sessionCount')}: ${detail?.sessionCount || 0}\n- ${t('agentDetailSub.messageCount')}: ${detail?.messageCount || 0}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'sessions': {
        return `## ${t('agentDetailSub.sessionList')} (${sessions.length})\n\n${sessions.map((s) => {
          const active = s.id === activeSession ? ' ' + t('agentDetailSub.currentSession') : ''
          const sys = isSystemSession(s.title) ? ' ' + t('agentDetailSub.systemLabel') : ''
          return `- ${s.title}${sys}${active}`
        }).join('\n')}`
      }

      case 'reset': {
        if (activeSession) {
          await invoke('clear_history', { sessionId: activeSession })
          setMessages([])
          return 'Session reset (history cleared, session preserved)'
        }
        return t('agentDetail.errorNoSession')
      }

      case 'stop': {
        if (streaming) {
          setStreaming(false)
          streamBuf.current = ''
          thinkingBuf.current = ''
          streamingSessionRef.current = ''
          sendingRef.current = false
          // 通知后端取消活跃的 agent loop
          if (activeSession) {
            invoke('stop_generation', { sessionId: activeSession }).catch((e) => console.warn('stop_generation failed:', e))
          }
          return 'Generation stopped'
        }
        return 'No active generation'
      }

      case 'export': {
        if (!activeSession || messages.length === 0) return t('agentDetail.noMessagesExport')
        const md = messages.map((m) => {
          if (m.role === 'user') return `${t('agentDetail.exportUser')} ${m.content}`
          if (m.role === 'assistant') return `${t('agentDetail.exportAssistant')} ${m.content}`
          if (m.role === 'tool') return `> [Tool] ${m.toolName}: ${m.content}`
          return `> ${m.content}`
        }).join('\n\n---\n\n')
        const title = sessions.find(s => s.id === activeSession)?.title || t('agentDetailSub.conversation')
        const blob = new Blob([`# ${title}\n\n${md}`], { type: 'text/markdown' })
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url; a.download = `${title}.md`; a.click()
        URL.revokeObjectURL(url)
        return t('agentDetailSub.exportedAs', { title })
      }

      case 'agents': {
        try {
          const list = await invoke<Agent[]>('list_agents')
          if (!list?.length) return t('agentDetailSub.noAgents')
          return `## ${t('agentDetailSub.agentList')} (${list.length})\n\n${list.map((a: Agent) =>
            `- **${a.name}** (\`${a.model}\`) ID: \`${a.id?.substring(0, 8)}...\``
          ).join('\n')}`
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      case 'kill': {
        if (!args.trim()) return t('agentDetailSub.killUsage')
        try {
          const subagents = await invoke<SubagentInfo[]>('list_subagents', { agentId })
          const running = subagents?.filter((s: SubagentInfo) => s.status === 'Running') || []
          if (running.length === 0) return t('agentDetailSub.noRunningSubagents')
          if (args.trim().toLowerCase() === 'all') {
            for (const sa of running) { await invoke('cancel_subagent', { subagentId: sa.id }) }
            return t('agentDetailSub.terminatedN', { n: running.length })
          }
          const target = running.find((s: SubagentInfo) => s.id.startsWith(args.trim()) || s.name === args.trim())
          if (!target) return t('agentDetailSub.notFoundSubagent') + ': ' + args.trim()
          await invoke('cancel_subagent', { subagentId: target.id })
          return t('agentDetailSub.terminatedSubagent', { name: target.name })
        } catch (e) { return t('agentDetailSub.terminateFailed') + ': ' + e }
      }

      case 'skill': {
        if (!args.trim()) {
          try {
            const list = await invoke<Skill[]>('list_skills', { agentId })
            if (!list?.length) return t('agentDetailSub.noSkills') + '. ' + t('agentDetailSub.skillUsageHint')
            return `${t('agentDetailSub.availableSkills')}:\n${list.map((s: Skill) => `- ${s.name} ${s.enabled ? '✓' : '✗'}`).join('\n')}\n\n${t('agentDetailSub.activateSkillHint')}`
          } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
        }
        // 激活技能（发送带技能关键词的消息给 LLM）
        return null // 返回 null 让消息走正常 LLM 流程，技能会被 skill_mgr.activate_for_message 匹配
      }

      case 'think': {
        const levels = ['off', 'minimal', 'low', 'medium', 'high']
        if (!args.trim() || !levels.includes(args.trim().toLowerCase())) {
          return t('agentDetailSub.thinkUsage', { levels: levels.join(' / ') })
        }
        try {
          // 通过 Agent config 存储 thinking level
          const detail = await invoke<any>('get_agent_detail', { agentId })
          const config = detail?.config ? JSON.parse(detail.config) : {}
          config.thinkingLevel = args.trim().toLowerCase()
          await invoke('update_agent', { agentId, config: JSON.stringify(config) })
          return t('agentDetailSub.thinkSet', { level: args.trim().toLowerCase() })
        } catch (e) { return t('agentDetailSub.setFailed', { error: String(e) }) }
      }

      case 'fast': {
        const arg = args.trim().toLowerCase()
        if (!arg || arg === 'status') {
          try {
            const detail = await invoke<any>('get_agent_detail', { agentId })
            const config = detail?.config ? JSON.parse(detail.config) : {}
            return t('agentDetailSub.fastStatus', { status: config.fastMode ? 'ON' : 'OFF' })
          } catch { return t('agentDetailSub.queryFailed') }
        }
        try {
          const detail = await invoke<any>('get_agent_detail', { agentId })
          const config = detail?.config ? JSON.parse(detail.config) : {}
          config.fastMode = arg === 'on'
          await invoke('update_agent', { agentId, config: JSON.stringify(config) })
          return t('agentDetailSub.fastToggled', { state: arg === 'on' ? t('agentDetailSub.fastOn') : t('agentDetailSub.fastOff') })
        } catch (e) { return t('agentDetailSub.setFailed', { error: String(e) }) }
      }

      case 'models': {
        try {
          const providers = await invoke<any[]>('get_providers')
          const lines: string[] = [t('agentDetailSub.availableModels')]
          for (const p of (providers || [])) {
            if (!p.enabled) continue
            const models = (p.models || []).map((m: any) => m.name || m.id).join(', ')
            if (models) {
              lines.push(`**${p.name}** (${p.apiType}): ${models}`)
            }
          }
          lines.push(t('agentDetailSub.modelSwitchHint'))
          return lines.join('\n')
        } catch (e) { return t('agentDetailSub.queryFailed') + ': ' + e }
      }

      default:
        return t('agentDetailSub.unknownSlashCmd', { cmd })
    }
  }

  const handleSend = async () => {
    if ((!input.trim() && pendingImages.length === 0) || streaming || !activeSession) return
    if (sendingRef.current) return
    sendingRef.current = true
    const userMsg = input.trim()
    setInput('')

    // 把待发送图片拼为 attachment 标记
    let fullMessage = userMsg
    if (pendingImages.length > 0) {
      const attachments = pendingImages.map(img => `[attachment:${img}]`).join('\n')
      fullMessage = fullMessage ? `${fullMessage}\n${attachments}` : attachments
      setPendingImages([])
    }

    // 斜杠命令拦截
    if (userMsg.startsWith('/')) {
      const spaceIdx = userMsg.indexOf(' ')
      const cmd = spaceIdx > 0 ? userMsg.substring(1, spaceIdx) : userMsg.substring(1)
      const cmdArgs = spaceIdx > 0 ? userMsg.substring(spaceIdx + 1) : ''
      const result = await handleSlashCommand(cmd.toLowerCase(), cmdArgs)
      if (result !== null) {
        // 命令处理了，显示结果
        setMessages((prev) => [...prev, { role: 'user', content: userMsg }])
        setMessages((prev) => [...prev, { role: 'system', content: result }])
        sendingRef.current = false
        return
      }
      // result === null: 命令要求走正常 LLM 流程（如 /skill <name>）
    }

    // 前端显示不含 base64（防止渲染卡死），只显示文字 + 图片标记
    const displayMsg = pendingImages.length > 0
      ? (userMsg ? `${userMsg}\n[${t('agentDetailSub.imageCount', { n: pendingImages.length })}]` : `[${t('agentDetailSub.imageCount', { n: pendingImages.length })}]`)
      : userMsg
    setMessages((prev) => [...prev, { role: 'user', content: displayMsg }])
    setMessages((prev) => [...prev, { role: 'assistant', content: '' }])
    streamingSessionRef.current = activeSession
    setHud({ round: 0, tokens: 0, cost: 0, lastTool: '', lastToolOk: true })
    setStreaming(true)
    streamBuf.current = ''

    try {
      if (chatMode === 'flash') {
        // Flash 模式：使用轻量级 send_chat_only，不带工具/技能/记忆
        await invoke<string>('send_chat_only', {
          agentId,
          sessionId: activeSession,
          message: fullMessage,
        })
      } else {
        // Standard / Thinking 模式：正常 send_message
        await invoke('send_message', {
          agentId,
          sessionId: activeSession,
          message: chatMode === 'thinking'
            ? `[${t('agentDetail.modeThinkingHint')}]\n\n${fullMessage}`
            : fullMessage,
        })
      }
      // invoke 完成意味着 orchestrator 已结束，兜底清除 streaming 状态
      // （llm-done 事件可能因竞态尚未到达）
      setStreaming(false)
      sendingRef.current = false
      // 从 DB 重新加载结构化消息（替换 streaming 临时状态，避免闪烁）
      loadMessagesRef.current()
    } catch (e) {
      setStreaming(false)
      sendingRef.current = false
      setMessages((prev) => [...prev, { role: 'system', content: String(e), isError: true }])
    }
  }

  // 编辑用户消息：更新内容并删除后续消息，然后重新发送
  const handleEditMessage = async (msgIdx: number, newContent: string) => {
    if (!activeSession || !agentId || streaming) return
    setEditingIdx(null)
    setEditingContent('')
    try {
      // 使用后端返回的实际 seq，避免 system 消息过滤导致的偏移问题
      const msgSeq = messages[msgIdx]?.seq
      if (msgSeq == null) { toast.error('无法确定消息序号'); return }
      await invoke('edit_message', { sessionId: activeSession, messageSeq: msgSeq, newContent })
      // 删除前端该消息之后的所有消息，更新当前消息内容
      setMessages(prev => {
        const updated = prev.slice(0, msgIdx)
        updated.push({ ...prev[msgIdx], content: newContent })
        return updated
      })
      // 重新发送编辑后的消息给 LLM
      setMessages(prev => [...prev, { role: 'assistant', content: '' }])
      streamingSessionRef.current = activeSession
      setStreaming(true)
      streamBuf.current = ''
      try {
        await invoke('send_message', { agentId, sessionId: activeSession, message: newContent })
        setStreaming(false)
        setMessages(prev => {
          if (prev.length > 0 && prev[prev.length - 1].role === 'assistant' && !prev[prev.length - 1].content) {
            return prev.slice(0, -1)
          }
          return prev
        })
      } catch (e) {
        setStreaming(false)
        setMessages(prev => [...prev, { role: 'system', content: String(e), isError: true }])
      }
    } catch (e) {
      toast.error(friendlyError(e))
    }
  }

  // 重新生成 AI 回复：创建分支 → 从该消息前的用户消息重新发送
  const handleRegenerate = async (msgIdx: number) => {
    if (!activeSession || !agentId || streaming) return
    let userMsgIdx = -1
    let userContent = ''
    for (let i = msgIdx - 1; i >= 0; i--) {
      if (messages[i].role === 'user') {
        userMsgIdx = i
        userContent = messages[i].content
        break
      }
    }
    if (userMsgIdx < 0 || !userContent) {
      toast.error(t('agentDetailSub.userMsgNotFound'))
      return
    }
    try {
      const msgSeq = messages[msgIdx]?.seq
      if (msgSeq == null) { toast.error('无法确定消息序号'); return }
      // 创建分支（保留原始对话，不删除）
      try {
        await invoke('fork_from_message', { sessionId: activeSession, messageSeq: msgSeq })
      } catch {} // fork 失败不影响重新生成
      await invoke('regenerate_response', { sessionId: activeSession, afterSeq: msgSeq })
      // 前端截断到该 AI 消息之前
      setMessages(prev => prev.slice(0, msgIdx))
      // 重新发送用户消息
      setMessages(prev => [...prev, { role: 'assistant', content: '' }])
      streamingSessionRef.current = activeSession
      setStreaming(true)
      streamBuf.current = ''
      try {
        await invoke('send_message', { agentId, sessionId: activeSession, message: userContent })
        setStreaming(false)
        setMessages(prev => {
          if (prev.length > 0 && prev[prev.length - 1].role === 'assistant' && !prev[prev.length - 1].content) {
            return prev.slice(0, -1)
          }
          return prev
        })
      } catch (e) {
        setStreaming(false)
        setMessages(prev => [...prev, { role: 'system', content: String(e), isError: true }])
      }
    } catch (e) {
      toast.error(friendlyError(e))
    }
  }

  return (
    <div style={{ display: 'flex', height: '100%', minHeight: 0 }}>
      {/* 会话列表 */}
      <div style={{ width: 200, minWidth: 200, flexShrink: 0, borderRight: '1px solid var(--border-subtle)', display: 'flex', flexDirection: 'column' }}>
        <div style={{ padding: 8, display: 'flex', flexDirection: 'column', gap: 4 }}>
          <button onClick={createSession} style={{
            width: '100%', padding: '8px', backgroundColor: 'var(--accent)', color: 'white',
            border: 'none', borderRadius: 4, cursor: 'pointer', fontSize: 13,
          }}>
            {t('agentDetail.newSession')}
          </button>
          <div style={{ display: 'flex', gap: 4 }}>
            <button onClick={() => { setSelectMode(!selectMode); setSelectedSessions(new Set()) }}
              style={{ flex: 1, padding: '4px', fontSize: 10, border: '1px solid var(--border-subtle)', borderRadius: 3, cursor: 'pointer', backgroundColor: selectMode ? 'var(--accent-bg)' : 'transparent', color: 'var(--text-muted)' }}>
              {selectMode ? t('agentDetail.cancelSelect') : t('agentDetail.batchSelect')}
            </button>
            {selectMode && selectedSessions.size > 0 && (
              <button onClick={batchDeleteSessions}
                style={{ flex: 1, padding: '4px', fontSize: 10, border: 'none', borderRadius: 3, cursor: 'pointer', backgroundColor: 'var(--error)', color: '#fff' }}>
                {t('agentDetail.batchDeleteBtn', { count: selectedSessions.size })}
              </button>
            )}
          </div>
          {/* 搜索框 */}
          <input
            value={sessionSearch}
            onChange={e => setSessionSearch(e.target.value)}
            placeholder={t('agentDetail.searchPlaceholder') || 'Search sessions...'}
            style={{
              width: '100%', padding: '5px 8px', border: '1px solid var(--border-subtle)',
              borderRadius: 4, fontSize: 11, boxSizing: 'border-box',
              backgroundColor: 'var(--bg-glass)', outline: 'none',
            }}
          />
        </div>
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {/* 用户对话 */}
          {sessions.filter(s => !isSystemSession(s.title) && (!sessionSearch || s.title.toLowerCase().includes(sessionSearch.toLowerCase()))).map((s) => (
            <div key={s.id} style={{ display: 'flex', alignItems: 'center' }}>
              {selectMode && (
                <input type="checkbox" checked={selectedSessions.has(s.id)}
                  onChange={() => toggleSessionSelect(s.id)}
                  style={{ margin: '0 4px 0 8px', cursor: 'pointer' }}
                />
              )}
              <div style={{ flex: 1 }}>
                <SessionItem s={s} activeSession={activeSession}
                  onSelect={() => { if (!selectMode) { setActiveSession(s.id); setMessages([]); setDisplayCount(50) } else { toggleSessionSelect(s.id) } }}
                  onDelete={() => deleteSession(s.id)}
                  onExport={() => exportSession(s.id)}
                  renamingSession={renamingSession} renameValue={renameValue}
                  setRenameValue={setRenameValue}
                  onStartRename={() => { setRenamingSession(s.id); setRenameValue(s.title) }}
                  onFinishRename={(v: string) => renameSession(s.id, v)}
                  onCancelRename={() => setRenamingSession('')}
                />
              </div>
            </div>
          ))}

          {/* 系统对话（cron/heartbeat）折叠区 */}
          {sessions.some(s => isSystemSession(s.title)) && (
            <>
              <div
                onClick={() => setShowSystemSessions(!showSystemSessions)}
                style={{
                  padding: '6px 12px', fontSize: 11, color: 'var(--text-muted)', cursor: 'pointer',
                  borderTop: '1px solid var(--border-subtle)', display: 'flex', alignItems: 'center', gap: 4,
                }}
              >
                <span style={{ fontSize: 10 }}>{showSystemSessions ? '▼' : '▶'}</span>
                {t('agentDetailSub.systemSessions')} ({sessions.filter(s => isSystemSession(s.title)).length})
                <span style={{ flex: 1 }} />
                <button
                  onClick={async (e) => {
                    e.stopPropagation()
                    if (!await showConfirm(t('agentDetailSub.cleanupConfirm'))) return
                    try {
                      const r = await invoke<any>('cleanup_system_sessions', { agentId, keepDays: 7 })
                      toast.success(t('agentDetailSub.cleanupDone', { sessions: r.deletedSessions, messages: r.deletedMessages }))
                      loadSessions()
                    } catch (err) { toast.error(t('agentDetailSub.cleanupFailed') + ': ' + err) }
                  }}
                  style={{ fontSize: 10, padding: '1px 6px', border: '1px solid var(--border-subtle)', borderRadius: 3, background: 'var(--bg-elevated)', cursor: 'pointer', color: 'var(--text-muted)' }}
                >
                  {t('agentDetailSub.cleanupBtn')}
                </button>
              </div>
              {showSystemSessions && sessions.filter(s => isSystemSession(s.title)).map((s) => (
                <SessionItem key={s.id} s={s} activeSession={activeSession}
                  onSelect={() => { setActiveSession(s.id); setMessages([]); setDisplayCount(50) }}
                  onDelete={() => deleteSession(s.id)}
                  renamingSession={renamingSession} renameValue={renameValue}
                  setRenameValue={setRenameValue}
                  onStartRename={() => { setRenamingSession(s.id); setRenameValue(s.title) }}
                  onFinishRename={(v: string) => renameSession(s.id, v)}
                  onCancelRename={() => setRenamingSession('')}
                  isSystem
                />
              ))}
            </>
          )}
        </div>
      </div>

      {/* 对话区（整个区域支持拖拽图片） */}
      <div
        style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'row', position: 'relative' }}
        onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); e.dataTransfer.dropEffect = 'copy' }}
        onDragEnter={(e) => { e.preventDefault(); e.stopPropagation() }}
        onDrop={(e) => { e.preventDefault(); e.stopPropagation(); handleDrop(e) }}
      >
      {/* 主对话区 */}
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        {!activeSession ? (
          <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
            {t('chat.selectConversation')}
          </div>
        ) : (
          <>
            {/* 对话内搜索栏 */}
            {chatSearchOpen && (
              <div style={{
                display: 'flex', alignItems: 'center', gap: 8, padding: '6px 12px',
                backgroundColor: 'var(--bg-elevated)', borderBottom: '1px solid var(--border-subtle)',
                flexShrink: 0,
              }}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>
                </svg>
                <input
                  ref={chatSearchInputRef}
                  type="text"
                  value={chatSearchQuery}
                  onChange={e => { setChatSearchQuery(e.target.value); setChatSearchIdx(0) }}
                  onKeyDown={e => {
                    if (e.key === 'Escape') { setChatSearchOpen(false); setChatSearchQuery(''); setChatSearchIdx(0) }
                    else if (e.key === 'Enter' && !e.shiftKey) {
                      e.preventDefault()
                      if (chatSearchMatches.length > 0) setChatSearchIdx(prev => (prev + 1) % chatSearchMatches.length)
                    }
                    else if (e.key === 'Enter' && e.shiftKey) {
                      e.preventDefault()
                      if (chatSearchMatches.length > 0) setChatSearchIdx(prev => (prev - 1 + chatSearchMatches.length) % chatSearchMatches.length)
                    }
                  }}
                  placeholder={t('chat.searchInChat')}
                  autoFocus
                  style={{
                    flex: 1, padding: '4px 8px', fontSize: 13, borderRadius: 4,
                    border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
                    color: 'var(--text-primary)', outline: 'none', minWidth: 0,
                  }}
                />
                <span style={{ fontSize: 12, color: 'var(--text-muted)', whiteSpace: 'nowrap', minWidth: 60, textAlign: 'center' }}>
                  {chatSearchQuery.trim()
                    ? chatSearchMatches.length > 0
                      ? t('chat.matchCount', { current: chatSearchIdx + 1, total: chatSearchMatches.length })
                      : t('chat.noMatches')
                    : ''}
                </span>
                <button
                  onClick={() => { if (chatSearchMatches.length > 0) setChatSearchIdx(prev => (prev - 1 + chatSearchMatches.length) % chatSearchMatches.length) }}
                  disabled={chatSearchMatches.length === 0}
                  style={{
                    padding: '2px 6px', border: '1px solid var(--border-subtle)', borderRadius: 4,
                    backgroundColor: 'transparent', color: 'var(--text-muted)', cursor: chatSearchMatches.length > 0 ? 'pointer' : 'default',
                    opacity: chatSearchMatches.length > 0 ? 1 : 0.4, fontSize: 12,
                  }}
                  title="Previous (Shift+Enter)"
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="18 15 12 9 6 15"/>
                  </svg>
                </button>
                <button
                  onClick={() => { if (chatSearchMatches.length > 0) setChatSearchIdx(prev => (prev + 1) % chatSearchMatches.length) }}
                  disabled={chatSearchMatches.length === 0}
                  style={{
                    padding: '2px 6px', border: '1px solid var(--border-subtle)', borderRadius: 4,
                    backgroundColor: 'transparent', color: 'var(--text-muted)', cursor: chatSearchMatches.length > 0 ? 'pointer' : 'default',
                    opacity: chatSearchMatches.length > 0 ? 1 : 0.4, fontSize: 12,
                  }}
                  title="Next (Enter)"
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polyline points="6 9 12 15 18 9"/>
                  </svg>
                </button>
                <button
                  onClick={() => { setChatSearchOpen(false); setChatSearchQuery(''); setChatSearchIdx(0) }}
                  style={{
                    padding: '2px 6px', border: 'none', borderRadius: 4,
                    backgroundColor: 'transparent', color: 'var(--text-muted)', cursor: 'pointer', fontSize: 12,
                  }}
                  title="Close (Esc)"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                  </svg>
                </button>
              </div>
            )}
            <div ref={messagesContainerRef} style={{ flex: 1, overflowY: 'auto', overflowX: 'hidden', padding: '16px 20px' }} onClick={handleCodeCopyClick}>
              {/* 消息分页：仅显示最近 displayCount 条，避免大量消息时渲染卡顿 */}
              {messages.length > displayCount && (
                <div style={{ textAlign: 'center', marginBottom: 12 }}>
                  <button
                    onClick={() => setDisplayCount(c => c + 50)}
                    style={{
                      padding: '6px 16px', fontSize: 12,
                      background: 'var(--bg-elevated)', color: 'var(--text-secondary)',
                      border: '1px solid var(--border-subtle)', borderRadius: 6,
                      cursor: 'pointer',
                    }}
                  >
                    {t('chat.loadMore') || '加载更早的消息'} ({messages.length - displayCount} {t('chat.messagesRemaining') || '条'})
                  </button>
                </div>
              )}
              {/* displayOffset: slice(-displayCount) 偏移，用于还原原始索引 */}
              {(() => { const displayOffset = Math.max(0, messages.length - displayCount); return groupMessages(messages.slice(-displayCount)).map((group) => {
                // 连续工具调用合并为一组
                if (group.type === 'tool-group') {
                  return (
                    <ToolGroupCard
                      key={`tg-${group.startIdx}`}
                      messages={group.messages}
                      groupKey={`tg-${group.startIdx}`}
                      expandedTools={expandedTools}
                      toggleTool={toggleTool}
                    />
                  )
                }

                const msg = group.messages[0]
                const i = group.startIdx
                const oi = i + displayOffset // 原始索引（对应完整 messages 数组）
                const isLastMsg = oi === messages.length - 1

                // 单个工具调用（保持现有卡片样式）
                if (msg.role === 'tool') {
                  const toolKey = `tool-${i}`
                  const isExpanded = expandedTools.has(toolKey)
                  const meta = extractToolMeta(msg.content || '')
                  const accentColor = meta.success ? 'var(--accent, #34d399)' : 'var(--error, #ef4444)'
                  const statusBg = meta.success ? 'rgba(52,211,153,0.1)' : 'rgba(239,68,68,0.1)'
                  return (
                    <div key={i} data-msg-idx={oi} style={{
                      marginBottom: 6, marginLeft: 38, maxWidth: 560,
                      borderRadius: 10, overflow: 'hidden',
                      border: '1px solid var(--border-subtle)',
                      borderLeft: `3px solid ${accentColor}`,
                      backgroundColor: 'var(--bg-elevated)',
                      transition: 'all 0.2s ease',
                    }}>
                      {/* 工具卡片头部 */}
                      <div
                        onClick={() => toggleTool(toolKey)}
                        style={{
                          padding: '8px 12px',
                          display: 'flex', alignItems: 'center', gap: 8,
                          cursor: 'pointer', userSelect: 'none',
                          backgroundColor: statusBg,
                        }}
                      >
                        <ToolGearIcon size={14} color={accentColor} />
                        <strong style={{ fontSize: 12, color: 'var(--text-primary)', fontFamily: "'SF Mono', Monaco, monospace" }}>
                          {msg.toolName || t('common.tools')}
                        </strong>
                        <div style={{ flex: 1 }} />
                        {/* 状态标记 */}
                        <span style={{
                          display: 'inline-flex', alignItems: 'center', gap: 4,
                          fontSize: 11, color: accentColor, fontWeight: 500,
                        }}>
                          {meta.success ? (
                            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={accentColor} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                              <polyline points="20 6 9 17 4 12"/>
                            </svg>
                          ) : (
                            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke={accentColor} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
                            </svg>
                          )}
                          {meta.success ? t('common.success') || 'Success' : t('common.failed') || 'Failed'}
                          {meta.duration && <span style={{ color: 'var(--text-muted)', fontWeight: 400 }}>{meta.duration}</span>}
                        </span>
                        {/* 展开/收起箭头 */}
                        <svg
                          width="14" height="14" viewBox="0 0 24 24" fill="none"
                          stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
                          style={{
                            transition: 'transform 0.2s ease',
                            transform: isExpanded ? 'rotate(180deg)' : 'rotate(0deg)',
                          }}
                        >
                          <polyline points="6 9 12 15 18 9"/>
                        </svg>
                      </div>
                      {/* 收起状态：一行预览 */}
                      {!isExpanded && msg.content && (
                        <div
                          onClick={() => toggleTool(toolKey)}
                          style={{
                            padding: '4px 12px 6px', cursor: 'pointer',
                            fontSize: 11, color: 'var(--text-muted)',
                            fontFamily: "'SF Mono', Monaco, monospace",
                            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                          }}
                        >
                          {msg.content.replace(/\n/g, ' ').slice(0, 100)}
                        </div>
                      )}
                      {/* 展开状态：完整内容 */}
                      {isExpanded && msg.content && (
                        <div style={{
                          padding: '8px 12px',
                          borderTop: '1px solid var(--border-subtle)',
                        }}>
                          <pre style={{
                            margin: 0, whiteSpace: 'pre-wrap', wordBreak: 'break-all',
                            fontFamily: "'SF Mono', Monaco, monospace", fontSize: 11, lineHeight: 1.5,
                            color: 'var(--text-secondary)',
                            backgroundColor: 'rgba(0,0,0,0.15)', borderRadius: 6,
                            padding: '8px 10px', maxHeight: 280, overflow: 'auto',
                          }}>
                            {formatToolContent(msg.content)}
                          </pre>
                        </div>
                      )}
                    </div>
                  )
                }

                // assistant 消息：分离内嵌的 [工具: xxx] 标记
                if (msg.role === 'assistant' && msg.content && typeof msg.content === 'string') {
                  const toolPattern = /\n?\[(?:工具|MCP 工具|技能工具): (.+?) 执行中\.\.\.\]\n?/g
                  const parts: Array<{ type: 'text' | 'tool'; content: string }> = []
                  let lastIdx = 0
                  let match
                  while ((match = toolPattern.exec(msg.content)) !== null) {
                    if (match.index > lastIdx) {
                      const text = msg.content.slice(lastIdx, match.index).trim()
                      if (text) parts.push({ type: 'text', content: text })
                    }
                    parts.push({ type: 'tool', content: match[1] })
                    lastIdx = match.index + match[0].length
                  }
                  if (lastIdx < msg.content.length) {
                    const text = msg.content.slice(lastIdx).trim()
                    if (text) parts.push({ type: 'text', content: text })
                  }

                  // 如果有工具标记，分段渲染（工具合并为一组，与完成后的 ToolGroupCard 样式一致）
                  const toolParts = parts.filter(p => p.type === 'tool')
                  const textParts = parts.filter(p => p.type === 'text')
                  if (toolParts.length > 0) {
                    return (
                      <div key={i} data-msg-idx={oi} className="msg-row message-enter" style={{
                        marginBottom: 12, display: 'flex', flexDirection: 'row',
                        alignItems: 'flex-start', gap: 8, overflow: 'hidden',
                      }}>
                        {/* 头像 */}
                        <div style={{
                          width: 30, height: 30, borderRadius: 8, flexShrink: 0, marginTop: 2,
                          background: 'rgba(52,211,153,0.08)',
                          display: 'flex', alignItems: 'center', justifyContent: 'center',
                          border: '1px solid rgba(52,211,153,0.15)',
                        }}>
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="9" cy="16" r="1" fill="var(--accent)" stroke="none"/><circle cx="15" cy="16" r="1" fill="var(--accent)" stroke="none"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>
                          </svg>
                        </div>
                        {/* 内容列 */}
                        <div style={{ display: 'flex', flexDirection: 'column', maxWidth: 560, minWidth: 0 }}>
                          {msg.thinking && <ThinkingBlock thinking={msg.thinking} />}
                          {/* 工具合并为一个组卡片（先于文字显示，与完成后顺序一致） */}
                          <div style={{
                            marginTop: 4, marginBottom: 6, maxWidth: 560,
                            borderRadius: 10, overflow: 'hidden',
                            border: '1px solid var(--border-subtle)',
                            borderLeft: '3px solid var(--accent, #34d399)',
                            backgroundColor: 'var(--bg-elevated)',
                          }}>
                            <div style={{
                              padding: '8px 12px',
                              display: 'flex', alignItems: 'center', gap: 8,
                              backgroundColor: 'rgba(52,211,153,0.06)',
                            }}>
                              <ToolGearIcon size={14} color="var(--accent, #34d399)" />
                              <strong style={{ fontSize: 12, color: 'var(--text-primary)', fontFamily: "'SF Mono', Monaco, monospace" }}>
                                {toolParts.length} {t('common.tools') || 'tools'}
                              </strong>
                              <div style={{ flex: 1 }} />
                              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 11, color: 'var(--accent, #34d399)', fontWeight: 500 }}>
                                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--accent, #34d399)" strokeWidth="2" strokeLinecap="round" style={{ animation: 'spin 1s linear infinite' }}>
                                  <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
                                </svg>
                                {t('common.running') || 'Running...'}
                              </span>
                            </div>
                            <div style={{ padding: '4px 12px 6px' }}>
                              {toolParts.map((tp, idx) => (
                                <div key={idx} style={{
                                  display: 'inline-flex', alignItems: 'center', gap: 4,
                                  marginRight: 10, fontSize: 11, color: 'var(--text-muted)',
                                  fontFamily: "'SF Mono', Monaco, monospace",
                                }}>
                                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="var(--accent, #34d399)" strokeWidth="2" strokeLinecap="round" style={{ animation: 'spin 1s linear infinite' }}>
                                    <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
                                  </svg>
                                  <span>{tp.content}</span>
                                </div>
                              ))}
                            </div>
                          </div>
                          {/* 文字部分（在工具卡片之后显示，与完成后顺序一致） */}
                          {textParts.map((part, pi) => (
                            <div key={`t-${pi}`} style={{
                              padding: '10px 14px', borderRadius: '12px 12px 12px 4px',
                              backgroundColor: 'var(--bg-elevated)', color: 'var(--text-primary)',
                              border: '1px solid var(--border-subtle)',
                              fontSize: 14, lineHeight: 1.6, wordBreak: 'break-word', overflowWrap: 'anywhere',
                              marginTop: 4,
                            }}>
                              {renderMd(part.content)}
                            </div>
                          ))}
                          {streaming && isLastMsg && <TypingIndicator />}
                        </div>
                      </div>
                    )
                  }
                }

                // 多 Agent 交互状态特殊渲染
                if (msg.role === 'assistant' && msg.content && typeof msg.content === 'string') {
                  const c = msg.content
                  // Yield 状态
                  if (c.includes('⏸️') || c.includes('YIELD:') || c.includes('⏳ Waiting for subagent')) {
                    return (
                      <div key={i} data-msg-idx={oi} style={{ marginBottom: 12, display: 'flex', gap: 8 }}>
                        <div style={{
                          padding: '10px 16px', borderRadius: 10, fontSize: 13,
                          backgroundColor: 'rgba(59,130,246,0.08)', border: '1px solid rgba(59,130,246,0.2)',
                          display: 'flex', alignItems: 'center', gap: 8,
                        }}>
                          <span style={{ color: 'var(--accent)' }}>{c.replace(/[⏸️⏳✅❌⚙️]/g, '').trim()}</span>
                        </div>
                      </div>
                    )
                  }
                  // Subagent Result
                  if (c.includes('[Subagent Result')) {
                    return (
                      <div key={i} data-msg-idx={oi} style={{ marginBottom: 12 }}>
                        <div style={{
                          padding: '10px 14px', borderRadius: 10, fontSize: 13,
                          backgroundColor: 'rgba(34,197,94,0.08)', border: '1px solid rgba(34,197,94,0.2)',
                        }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
                            <strong style={{ color: 'var(--success)' }}>Subagent Result</strong>
                          </div>
                          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.6 }}>{renderMd(c.replace(/\[Subagent Result.*?\]\n*/, ''))}</div>
                        </div>
                      </div>
                    )
                  }
                  // A2A 对话结果
                  if (c.includes('A2A conversation with')) {
                    const lines = c.split('\n').filter(Boolean)
                    const header = lines[0] || ''
                    const turns = lines.slice(1)
                    return (
                      <div key={i} data-msg-idx={oi} style={{ marginBottom: 12 }}>
                        <div style={{
                          padding: '10px 14px', borderRadius: 10, fontSize: 13,
                          backgroundColor: 'rgba(139,92,246,0.08)', border: '1px solid rgba(139,92,246,0.2)',
                        }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
                            <strong style={{ color: '#8b5cf6' }}>{header}</strong>
                          </div>
                          {turns.map((line, li) => (
                            <div key={li} style={{
                              padding: '4px 8px', marginBottom: 2, fontSize: 12,
                              backgroundColor: line.includes('You →') ? 'rgba(59,130,246,0.06)' : 'rgba(34,197,94,0.06)',
                              borderRadius: 4,
                            }}>
                              {line}
                            </div>
                          ))}
                        </div>
                      </div>
                    )
                  }
                  // Auto-compact 通知
                  if (c.includes('Context overflow') || c.includes('auto-compacting') || c.includes('Auto-compacted')) {
                    return (
                      <div key={i} data-msg-idx={oi} style={{ marginBottom: 12, display: 'flex', gap: 8 }}>
                        <div style={{
                          padding: '8px 14px', borderRadius: 8, fontSize: 12,
                          backgroundColor: 'rgba(251,191,36,0.1)', border: '1px solid rgba(251,191,36,0.2)',
                          color: '#d97706',
                        }}>
                          {c.replace(/[⚙️]/g, '').trim()}
                        </div>
                      </div>
                    )
                  }
                }

                const isUser = msg.role === 'user'
                const isSystem = msg.role === 'system'
                const isErrorMsg = !!(msg.isError)
                // 检测错误消息中是否包含需要跳转设置的关键词
                const needsSettingsLink = isErrorMsg && /设置|Key|供应商|充值|过期/.test(msg.content)

                return (
                  <div key={i} data-msg-idx={oi} style={{
                    marginBottom: 12, display: 'flex',
                    flexDirection: isUser ? 'row-reverse' : 'row',
                    alignItems: 'flex-start', gap: 8,
                    overflow: 'hidden',
                    ...(chatSearchOpen && chatSearchMatches.includes(oi) ? {
                      boxShadow: chatSearchMatches[chatSearchIdx] === oi
                        ? '0 0 0 2px var(--accent)' : '0 0 0 1px var(--warning, #f59e0b)',
                      borderRadius: 12,
                    } : {}),
                  }}
                    className="msg-row message-enter"
                  >
                    {/* 头像 */}
                    <div style={{
                      width: 30, height: 30, borderRadius: 8, flexShrink: 0, marginTop: 2,
                      background: isUser ? 'var(--accent)' : isErrorMsg ? 'rgba(239,68,68,0.1)' : 'rgba(52,211,153,0.08)',
                      display: 'flex', alignItems: 'center', justifyContent: 'center',
                      border: isUser ? 'none' : isErrorMsg ? '1px solid rgba(239,68,68,0.2)' : '1px solid rgba(52,211,153,0.15)',
                    }}>
                      {isUser ? (
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>
                        </svg>
                      ) : isErrorMsg ? (
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#ef4444" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                          <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
                        </svg>
                      ) : (
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                          <rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="9" cy="16" r="1" fill="var(--accent)" stroke="none"/><circle cx="15" cy="16" r="1" fill="var(--accent)" stroke="none"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>
                        </svg>
                      )}
                    </div>
                    {/* 消息内容列 */}
                    <div className="agent-msg-bubble" style={{ display: 'flex', flexDirection: 'column', maxWidth: 560, minWidth: 0 }}>
                    {/* 推理过程折叠显示（assistant 消息） */}
                    {msg.role === 'assistant' && msg.thinking && <ThinkingBlock thinking={msg.thinking} />}
                    {/* 等待中：直接渲染 TypingIndicator，不套气泡（避免双层背景导致点不可见） */}
                    {streaming && isLastMsg && !msg.content && !msg.thinking
                      ? <TypingIndicator />
                      : <div style={{
                      padding: '10px 14px', borderRadius: isUser ? '12px 12px 4px 12px' : '12px 12px 12px 4px',
                      backgroundColor: isUser ? 'var(--accent)' : isErrorMsg ? 'rgba(239,68,68,0.06)' : isSystem ? 'var(--success-bg)' : 'var(--bg-elevated)',
                      color: isUser ? '#fff' : isErrorMsg ? '#ef4444' : 'var(--text-primary)',
                      border: isUser ? 'none' : isErrorMsg ? '1px solid rgba(239,68,68,0.2)' : '1px solid var(--border-subtle)',
                      borderLeft: isErrorMsg ? '3px solid #ef4444' : undefined,
                      fontSize: isSystem ? 13 : 14,
                      lineHeight: 1.6, wordBreak: 'break-word', overflowWrap: 'anywhere',
                    }}>
                      {/* 用户消息编辑模式 */}
                      {isUser && editingIdx === oi ? (
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                          <textarea
                            value={editingContent}
                            onChange={e => setEditingContent(e.target.value)}
                            autoFocus
                            style={{
                              width: '100%', minHeight: 60, padding: 8, borderRadius: 6,
                              border: '1px solid rgba(255,255,255,0.3)', backgroundColor: 'rgba(0,0,0,0.1)',
                              color: '#fff', fontSize: 14, resize: 'vertical', outline: 'none',
                              fontFamily: 'inherit', lineHeight: 1.5,
                            }}
                            onKeyDown={e => {
                              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
                                e.preventDefault()
                                if (editingContent.trim()) handleEditMessage(oi, editingContent.trim())
                              }
                              if (e.key === 'Escape') { setEditingIdx(null); setEditingContent('') }
                            }}
                          />
                          <div style={{ display: 'flex', gap: 6, justifyContent: 'flex-end' }}>
                            <button
                              onClick={() => { setEditingIdx(null); setEditingContent('') }}
                              style={{
                                padding: '3px 10px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                                border: '1px solid rgba(255,255,255,0.3)', backgroundColor: 'transparent', color: '#fff',
                              }}
                            >Cancel</button>
                            <button
                              onClick={() => { if (editingContent.trim()) handleEditMessage(oi, editingContent.trim()) }}
                              style={{
                                padding: '3px 10px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                                border: 'none', backgroundColor: 'rgba(255,255,255,0.2)', color: '#fff', fontWeight: 600,
                              }}
                            aria-label="发送消息">Send</button>
                          </div>
                        </div>
                      ) : isErrorMsg ? (
                        <div>
                          <div>{msg.content}</div>
                          <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                            {needsSettingsLink && (
                              <button
                                onClick={() => { window.location.hash = '#/settings' }}
                                style={{
                                  padding: '4px 12px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                                  border: '1px solid rgba(239,68,68,0.3)', backgroundColor: 'rgba(239,68,68,0.08)',
                                  color: '#ef4444', fontWeight: 500,
                                }}
                              >{t('agentDetailSub.goToSettings')}</button>
                            )}
                            <button
                              onClick={async () => {
                                if (!activeSession || !agentId || streaming) return
                                // 找到最后一条用户消息
                                const lastUserMsg = messages.slice(0, oi).reverse().find(m => m.role === 'user')
                                if (!lastUserMsg) return
                                // 移除错误消息，添加空 assistant 占位
                                setMessages(prev => [...prev.filter(m => m !== msg), { role: 'assistant', content: '' }])
                                setStreaming(true)
                                streamBuf.current = ''
                                try {
                                  await invoke('send_message', { agentId, sessionId: activeSession, message: lastUserMsg.content })
                                  setStreaming(false)
                                  loadMessagesRef.current()
                                } catch (e2) {
                                  setStreaming(false)
                                  setMessages(prev => [...prev, { role: 'system', content: String(e2), isError: true }])
                                }
                              }}
                              style={{
                                padding: '4px 12px', fontSize: 12, borderRadius: 4, cursor: 'pointer',
                                border: '1px solid rgba(239,68,68,0.3)', backgroundColor: 'transparent',
                                color: '#ef4444', fontWeight: 500,
                              }}
                            >{t('agentDetailSub.retry')}</button>
                          </div>
                        </div>
                      ) : (msg.role === 'assistant' || isSystem) ? (
                        msg.content
                          ? renderMd(typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content))
                          : streaming && oi === messages.length - 1
                            ? <TypingIndicator />
                            : null
                      ) : (
                        <UserMessageContent content={typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content || '')} searchQuery={chatSearchOpen ? chatSearchQuery : undefined} />
                      )}
                    </div>}
                    {/* 操作按钮（消息下方，hover 显示） */}
                    <div className="msg-actions" style={{
                      gap: 4, alignSelf: isUser ? 'flex-end' : 'flex-start',
                    }}>
                      {/* 用户消息：编辑 */}
                      {isUser && !streaming && editingIdx !== oi && (
                        <button
                          onClick={() => { setEditingIdx(oi); setEditingContent(typeof msg.content === 'string' ? msg.content : '') }}
                          title="Edit" style={actionBtnStyle}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/>
                          </svg>
                        </button>
                      )}
                      {/* AI 消息：重新生成 + 分支导航 + 反馈 */}
                      {!isUser && !isSystem && msg.content && !streaming && (<>
                        <button onClick={() => handleRegenerate(oi)} title="重新生成（创建新分支）" aria-label="重新生成回复" style={actionBtnStyle}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/>
                          </svg>
                        </button>
                        {/* 分支导航（有分支数据时显示） */}
                        {(msg as any).branchId && (msg as any).branchId !== 'main' && (
                          <span style={{ fontSize: 10, color: 'var(--text-muted)', padding: '0 4px', display: 'inline-flex', alignItems: 'center', gap: 2 }}>
                            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M6 3v12"/><path d="M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/><path d="M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z"/><path d="M18 9c0 6-6 9-12 9"/></svg>
                            分支
                          </span>
                        )}
                        <button onClick={() => msg.seq != null && invoke('submit_message_feedback', { sessionId: activeSession, messageSeq: msg.seq, feedback: 'up' }).then(() => toast.success('Thanks!')).catch((e) => console.warn('feedback submit failed:', e))}
                          title="Good" style={actionBtnStyle}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M14 9V5a3 3 0 0 0-3-3l-4 9v11h11.28a2 2 0 0 0 2-1.7l1.38-9a2 2 0 0 0-2-2.3H14z"/><path d="M4 22H2V11h2"/>
                          </svg>
                        </button>
                        <button onClick={() => msg.seq != null && invoke('submit_message_feedback', { sessionId: activeSession, messageSeq: msg.seq, feedback: 'down' }).then(() => toast.success('Noted')).catch((e) => console.warn('feedback submit failed:', e))}
                          title="Bad" style={actionBtnStyle}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <path d="M10 15v4a3 3 0 0 0 3 3l4-9V2H5.72a2 2 0 0 0-2 1.7l-1.38 9a2 2 0 0 0 2 2.3H10z"/><path d="M20 2h2v11h-2"/>
                          </svg>
                        </button>
                        <button
                          onClick={() => {
                            if (isSpeaking) { stopSpeaking() } else { speak(typeof msg.content === 'string' ? msg.content : String(msg.content)) }
                          }}
                          title={isSpeaking ? t('voice.stopSpeaking') : t('voice.speakMessage')} style={actionBtnStyle}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                            <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>
                            <path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>
                          </svg>
                        </button>
                      </>)}
                    </div>
                    </div>{/* 关闭消息内容列 */}
                  </div>
                )
              })})()}
              <div ref={messagesEndRef} />
            </div>
            {/* 功能栏 */}
            <ToolBar
              messageCount={messages.filter(m => m.role !== 'system').length}
              showCompact={messages.length > 20}
              onCompact={async () => {
                if (!agentId || !activeSession) {
                  throw new Error('No active session')
                }
                // 直接调用后端压缩，ToolBar 自身显示 loading 状态
                const r = await invoke<string>('compact_session', { agentId, sessionId: activeSession })
                // 压缩成功后重新加载消息
                await loadMessages()
                toast.success(t('agentDetail.compactDone') || 'Compacted')
                return r
              }}
              agentId={agentId}
              sessionId={activeSession}
              streaming={streaming}
              hud={hud}
            />
            {/* 命令提示 */}
            {input.startsWith('/') && !input.includes(' ') && (() => {
              const SLASH_CMDS: Array<[string, string, boolean]> = [
                ['/help', 'Show all commands', false],
                ['/new', 'New session', false],
                ['/model', 'Switch model (e.g. /model gpt-4o)', true],
                ['/fast', 'Toggle fast mode', false],
                ['/think', 'Toggle thinking/reasoning', false],
                ['/status', 'Agent status', false],
                ['/usage', 'Token usage stats', false],
                ['/tools', 'List available tools', false],
                ['/skills', 'List installed skills', false],
                ['/skill', 'Install skill (e.g. /skill search)', true],
                ['/providers', 'List LLM providers', false],
                ['/memory', 'Query memory', false],
                ['/compact', 'Compress history', false],
                ['/clear', 'Clear session', false],
                ['/reset', 'Reset agent', false],
                ['/export', 'Export session', false],
                ['/rename', 'Rename session', true],
                ['/sessions', 'List sessions', false],
                ['/stop', 'Stop generation', false],
                ['/agents', 'List agents', false],
                ['/kill', 'Kill subprocess', true],
                ['/doctor', 'System diagnostics', false],
                ['/search', 'Search messages', true],
                ['/browser', 'Browser automation', true],
              ]
              const filtered = SLASH_CMDS.filter(([cmd]) => cmd.startsWith(input.toLowerCase()))
              return filtered.length > 0 ? (
                <div style={{
                  padding: '8px 12px', borderTop: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
                  fontSize: 12, maxHeight: 200, overflowY: 'auto',
                }}>
                  {filtered.map(([cmd, desc, needsArg]) => (
                    <div key={cmd}
                      onClick={() => setInput(needsArg ? cmd + ' ' : cmd)}
                      style={{
                        cursor: 'pointer', padding: '5px 8px', borderRadius: 4,
                        display: 'flex', justifyContent: 'space-between', gap: 12,
                      }}
                      onMouseEnter={e => { (e.currentTarget as HTMLElement).style.backgroundColor = 'var(--accent-bg)' }}
                      onMouseLeave={e => { (e.currentTarget as HTMLElement).style.backgroundColor = 'transparent' }}
                    >
                      <span style={{ fontFamily: 'monospace', color: 'var(--accent)', fontWeight: 600 }}>{cmd}</span>
                      <span style={{ color: 'var(--text-muted)' }}>{desc}</span>
                    </div>
                  ))}
                </div>
              ) : null
            })()}
            {/* 附件预览 */}
            {pendingImages.length > 0 && (
              <div style={{ padding: '6px 16px', borderTop: '1px solid var(--border-subtle)', display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                {pendingImages.map((img, idx) => (
                  <div key={idx} style={{ position: 'relative', display: 'inline-block' }}>
                    <img src={img} alt="" style={{ height: 48, borderRadius: 6, border: '1px solid var(--border-subtle)' }} />
                    <button
                      onClick={() => setPendingImages(prev => prev.filter((_, i) => i !== idx))}
                      style={{
                        position: 'absolute', top: -6, right: -6, width: 18, height: 18,
                        borderRadius: '50%', backgroundColor: '#ef4444', color: 'white',
                        border: 'none', fontSize: 10, cursor: 'pointer', lineHeight: '18px', padding: 0,
                      }}
                    >x</button>
                  </div>
                ))}
              </div>
            )}
            {/* 模式选择器 */}
            <div style={{
              display: 'flex', gap: 2, padding: '4px 16px',
              borderTop: '1px solid var(--border-subtle)',
            }}>
              {([
                { key: 'flash' as const, labelKey: 'agentDetail.modeFlash',
                  icon: <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z"/></svg> },
                { key: 'standard' as const, labelKey: 'agentDetail.modeStandard',
                  icon: <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg> },
                { key: 'thinking' as const, labelKey: 'agentDetail.modeThinking',
                  icon: <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z"/></svg> },
              ]).map(m => (
                <button key={m.key} onClick={() => setChatMode(m.key)}
                  style={{
                    padding: '3px 10px', fontSize: 11, borderRadius: 6,
                    border: chatMode === m.key ? '1px solid var(--accent)' : '1px solid transparent',
                    background: chatMode === m.key ? 'var(--accent-bg)' : 'transparent',
                    color: chatMode === m.key ? 'var(--accent)' : 'var(--text-muted)',
                    cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4,
                    transition: 'all 0.15s ease',
                  }}
                >
                  {m.icon}
                  {t(m.labelKey)}
                </button>
              ))}
            </div>
            {/* 输入区 */}
            <div
              style={{ padding: '10px 16px', borderTop: '1px solid var(--border-subtle)' }}
              onDrop={handleDrop}
              onDragOver={(e) => e.preventDefault()}
            >
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <button
                  onClick={() => fileInputRef.current?.click()}
                  disabled={streaming}
                  title={t('agentDetailSub.addImage')}
                  style={{
                    padding: '8px', backgroundColor: 'transparent', border: '1px solid #d1d5db',
                    borderRadius: 6, cursor: 'pointer', fontSize: 16, lineHeight: 1, color: 'var(--text-secondary)', flexShrink: 0,
                  }}
                >+</button>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  multiple
                  style={{ display: 'none' }}
                  onChange={(e) => { if (e.target.files) addImageFiles(e.target.files); e.target.value = '' }}
                />
                {/* 麦克风按钮 */}
                <button
                  onClick={async () => {
                    if (isRecording) {
                      const text = await stopRecording()
                      if (text) setInput(prev => prev ? prev + ' ' + text : text)
                    } else if (isTranscribing) {
                      // 识别中不响应
                    } else {
                      startRecording()
                    }
                  }}
                  onContextMenu={(e) => { e.preventDefault(); if (isRecording) cancelRecording() }}
                  disabled={streaming}
                  title={isRecording ? t('voice.stopRecording') : isTranscribing ? t('voice.transcribing') : t('voice.startRecording')}
                  style={{
                    padding: '8px', backgroundColor: isRecording ? '#ef4444' : 'transparent',
                    border: isRecording ? '1px solid #ef4444' : '1px solid #d1d5db',
                    borderRadius: 6, cursor: streaming ? 'not-allowed' : 'pointer',
                    fontSize: 16, lineHeight: 1, color: isRecording ? '#fff' : 'var(--text-secondary)',
                    flexShrink: 0, position: 'relative', display: 'flex', alignItems: 'center', justifyContent: 'center',
                    width: 36, height: 36,
                  }}
                >
                  {isTranscribing ? (
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ animation: 'spin 1s linear infinite' }}>
                      <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
                    </svg>
                  ) : (
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/>
                      <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
                      <line x1="12" y1="19" x2="12" y2="23"/>
                      <line x1="8" y1="23" x2="16" y2="23"/>
                    </svg>
                  )}
                  {isRecording && (
                    <span style={{
                      position: 'absolute', top: -6, right: -6, fontSize: 10, backgroundColor: '#ef4444',
                      color: '#fff', borderRadius: 8, padding: '1px 5px', fontWeight: 600, minWidth: 20, textAlign: 'center',
                    }}>{recordingDuration}s</span>
                  )}
                </button>
                <style>{`@keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }`}</style>
                <input
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onCompositionStart={() => { composingRef.current = true }}
                  onCompositionEnd={() => {
                    // Windows WebView2 事件顺序：compositionEnd → keyDown
                    // 延迟重置，确保紧接的 keyDown 还能检测到组合状态
                    setTimeout(() => { composingRef.current = false }, 50)
                  }}
                  onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey && !composingRef.current && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); handleSend() } }}
                  onPaste={handlePaste}
                  placeholder={t('agentDetail.inputHint')}
                  disabled={streaming}
                  style={{ flex: 1, padding: '10px', border: '1px solid var(--border-subtle)', borderRadius: 6, fontSize: 14 }}
                />
                <button
                  onClick={handleSend}
                  disabled={streaming || (!input.trim() && pendingImages.length === 0)}
                  style={{
                    padding: '10px 20px', backgroundColor: 'var(--accent)', color: 'white',
                    border: 'none', borderRadius: 6, cursor: streaming || (!input.trim() && pendingImages.length === 0) ? 'not-allowed' : 'pointer',
                    opacity: streaming || (!input.trim() && pendingImages.length === 0) ? 0.6 : 1,
                  }}
                >
                  {streaming ? t('agentDetail.generating') : t('common.send')}
                </button>
                {/* 语音朗读开关 */}
                <button
                  onClick={() => {
                    const next = !voiceEnabled
                    setVoiceEnabled(next)
                    toast.success(next ? t('voice.voiceOn') : t('voice.voiceOff'))
                  }}
                  title={voiceEnabled ? t('voice.voiceOn') : t('voice.voiceOff')}
                  style={{
                    padding: '8px', backgroundColor: voiceEnabled ? 'var(--accent)' : 'transparent',
                    border: '1px solid var(--border-subtle)', borderRadius: 6,
                    cursor: 'pointer', lineHeight: 1,
                    color: voiceEnabled ? '#fff' : 'var(--text-secondary)', flexShrink: 0,
                    transition: 'all 0.2s', display: 'flex', alignItems: 'center', justifyContent: 'center',
                    width: 36, height: 36,
                  }}
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>
                    {voiceEnabled ? (
                      <>
                        <path d="M19.07 4.93a10 10 0 0 1 0 14.14"/>
                        <path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>
                      </>
                    ) : (
                      <line x1="23" y1="9" x2="17" y2="15"/>
                    )}
                  </svg>
                </button>
                {/* 多 Agent 面板切换按钮 */}
                <button
                  onClick={() => setShowAgentPanel(!showAgentPanel)}
                  title={t('multiAgent.panelTitle')}
                  style={{
                    padding: '8px', backgroundColor: showAgentPanel ? 'var(--accent)' : 'transparent',
                    border: '1px solid var(--border-subtle)', borderRadius: 6,
                    cursor: 'pointer', fontSize: 16, lineHeight: 1,
                    color: showAgentPanel ? '#fff' : 'var(--text-secondary)', flexShrink: 0,
                    transition: 'all 0.2s',
                  }}
                >A+</button>
              </div>
            </div>
          </>
        )}
      </div>
      {/* 多 Agent 协作面板（右侧） */}
      {showAgentPanel && activeSession && (
        <div style={{
          width: 280, borderLeft: '1px solid var(--border-subtle)', display: 'flex', flexDirection: 'column',
          backgroundColor: 'var(--bg-elevated)', overflow: 'hidden', flexShrink: 0,
        }}>
          <div style={{
            padding: '10px 14px', borderBottom: '1px solid var(--border-subtle)',
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          }}>
            <span style={{ fontSize: 14, fontWeight: 600 }}>{t('multiAgent.panelTitle')}</span>
            <button onClick={() => setShowAgentPanel(false)}
              style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: 16, color: 'var(--text-muted)' }}>×</button>
          </div>

          <div style={{ flex: 1, overflowY: 'auto', padding: 12 }}>
            {/* 活跃子 Agent */}
            {activeSubagents.filter(s => s.status === 'Running' || s.status === 'Pending').length > 0 && (
              <div style={{ marginBottom: 16 }}>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                  {t('multiAgent.activeSubagents')}
                </div>
                {activeSubagents.filter(s => s.status === 'Running' || s.status === 'Pending').map(sa => (
                  <div key={sa.id} style={{
                    padding: '8px 10px', marginBottom: 4, borderRadius: 6,
                    backgroundColor: 'rgba(59,130,246,0.08)', border: '1px solid rgba(59,130,246,0.15)',
                    fontSize: 12,
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                      <span style={{
                        width: 6, height: 6, borderRadius: '50%',
                        backgroundColor: sa.status === 'Running' ? '#22c55e' : '#f0ad4e',
                        animation: sa.status === 'Running' ? 'pulse 1.5s ease-in-out infinite' : 'none',
                      }} />
                      <strong style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {sa.name}
                      </strong>
                      <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>{sa.status}</span>
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {sa.task}
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* 邮箱消息 */}
            {mailboxMsgs.length > 0 && (
              <div style={{ marginBottom: 16 }}>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                  {t('multiAgent.mailbox')} ({mailboxMsgs.length})
                </div>
                {mailboxMsgs.slice(-5).map((msg, i) => (
                  <div key={i} style={{
                    padding: '6px 10px', marginBottom: 3, borderRadius: 6,
                    backgroundColor: 'rgba(139,92,246,0.08)', border: '1px solid rgba(139,92,246,0.15)',
                    fontSize: 11,
                  }}>
                    <div style={{ fontWeight: 500, color: '#8b5cf6' }}>
                      {otherAgents.find(a => a.id === msg.from)?.name || msg.from?.slice(0, 8)}
                    </div>
                    <div style={{ color: 'var(--text-secondary)', marginTop: 2 }}>{msg.content}</div>
                  </div>
                ))}
              </div>
            )}

            {/* 可用 Agent 列表 */}
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                {t('multiAgent.availableAgents')}
              </div>
              {otherAgents.length === 0 ? (
                <div style={{ fontSize: 12, color: 'var(--text-muted)', padding: '8px 0' }}>{t('multiAgent.noOtherAgents')}</div>
              ) : (
                otherAgents.map(a => (
                  <div key={a.id} style={{
                    padding: '8px 10px', marginBottom: 4, borderRadius: 6,
                    border: '1px solid var(--border-subtle)', backgroundColor: 'var(--bg-glass)',
                    display: 'flex', alignItems: 'center', gap: 8,
                  }}>
                    <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)' }}>AI</span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: 13, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.name}</div>
                      <div style={{ fontSize: 10, color: 'var(--text-muted)' }}>{a.model}</div>
                    </div>
                    <button
                      onClick={() => handleInviteAgent(a.id)}
                      title={t('multiAgent.invite')}
                      style={{
                        padding: '3px 8px', fontSize: 11, borderRadius: 4, border: '1px solid var(--border-subtle)',
                        backgroundColor: 'var(--bg-elevated)', cursor: 'pointer', color: 'var(--accent)', flexShrink: 0,
                      }}
                    >
                      {t('multiAgent.invite')}
                    </button>
                  </div>
                ))
              )}
            </div>

            {/* 发送 Agent 间消息 */}
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                {t('multiAgent.sendMessage')}
              </div>
              <Select value={agentMsgTarget} onChange={setAgentMsgTarget}
                placeholder={t('multiAgent.selectTarget')}
                options={otherAgents.map(a => ({ value: a.id, label: a.name }))}
                style={{ width: '100%', marginBottom: 4 }} />
              <div style={{ display: 'flex', gap: 4 }}>
                <input value={agentMsgContent} onChange={e => setAgentMsgContent(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) handleSendAgentMsg() }}
                  placeholder={t('multiAgent.msgPlaceholder')}
                  style={{ flex: 1, padding: '6px 8px', borderRadius: 4, border: '1px solid var(--border-subtle)', fontSize: 12 }}
                />
                <button onClick={handleSendAgentMsg} disabled={!agentMsgTarget || !agentMsgContent.trim()}
                  style={{ padding: '6px 10px', fontSize: 11, borderRadius: 4, border: 'none', backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer', flexShrink: 0 }}>
                  {t('common.send')}
                </button>
              </div>
            </div>

            {/* A2A 对话 */}
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                {t('multiAgent.a2aChat')}
              </div>
              <Select value={a2aTarget} onChange={setA2aTarget}
                placeholder={t('multiAgent.selectTarget')}
                options={otherAgents.map(a => ({ value: a.id, label: a.name }))}
                style={{ width: '100%', marginBottom: 4 }} />
              <div style={{ display: 'flex', gap: 4 }}>
                <input value={a2aTopic} onChange={e => setA2aTopic(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing && e.keyCode !== 229) handleA2aChat() }}
                  placeholder={t('multiAgent.topicPlaceholder')}
                  style={{ flex: 1, padding: '6px 8px', borderRadius: 4, border: '1px solid var(--border-subtle)', fontSize: 12 }}
                />
                <button onClick={handleA2aChat} disabled={!a2aTarget || !a2aTopic.trim()}
                  style={{ padding: '6px 10px', fontSize: 11, borderRadius: 4, border: 'none', backgroundColor: '#8b5cf6', color: '#fff', cursor: 'pointer', flexShrink: 0 }}>
                  {t('multiAgent.startChat')}
                </button>
              </div>
            </div>

            {/* 已完成的子 Agent */}
            {activeSubagents.filter(s => s.status !== 'Running' && s.status !== 'Pending').length > 0 && (
              <div>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6, textTransform: 'uppercase' }}>
                  {t('multiAgent.completedSubagents')}
                </div>
                {activeSubagents.filter(s => s.status !== 'Running' && s.status !== 'Pending').slice(-5).map(sa => (
                  <div key={sa.id} style={{
                    padding: '6px 10px', marginBottom: 3, borderRadius: 6,
                    border: '1px solid var(--border-subtle)', fontSize: 11, opacity: 0.7,
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                      <span style={{ color: sa.status === 'Completed' ? '#22c55e' : '#ef4444' }}>
                        {sa.status === 'Completed' ? '\u2713' : '\u2717'}
                      </span>
                      <span style={{ fontWeight: 500 }}>{sa.name}</span>
                    </div>
                    {sa.result && (
                      <div style={{ color: 'var(--text-muted)', marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {sa.result.slice(0, 60)}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
      </div>
    </div>
  )
}
