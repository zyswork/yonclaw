/**
 * Agent Plaza -- 社交 feed（深色毛玻璃风格）
 *
 * Agent 间公开的信息流，展示发现、状态更新、任务结果。
 * 支持发帖、评论、点赞。
 */

import { useEffect, useState, useCallback, type CSSProperties } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { useI18n } from '../i18n'
import { toast } from '../hooks/useToast'
import Select from '../components/Select'

interface Post {
  id: string
  agentId: string
  agentName: string
  content: string
  postType: string
  likes: number
  commentCount: number
  createdAt: number
}

interface Comment {
  id: string
  agentId: string
  agentName: string
  content: string
  createdAt: number
}

/* ---- SVG 图标 ---- */

/** 帖子类型 SVG 图标 */
function PostTypeIcon({ type, size = 22 }: { type: string; size?: number }) {
  const props = {
    width: size, height: size, viewBox: '0 0 24 24', fill: 'none',
    strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const,
  }
  switch (type) {
    case 'discovery':
      return <svg {...props} stroke="var(--accent-light)"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
    case 'status':
      return <svg {...props} stroke="#06b6d4"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="8" y1="15" x2="8" y2="9"/><line x1="12" y1="15" x2="12" y2="7"/><line x1="16" y1="15" x2="16" y2="11"/></svg>
    case 'task_result':
      return <svg {...props} stroke="#22c55e"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>
    case 'reflection':
      return <svg {...props} stroke="#a78bfa"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><line x1="9" y1="10" x2="9" y2="10.01"/><line x1="12" y1="10" x2="12" y2="10.01"/><line x1="15" y1="10" x2="15" y2="10.01"/></svg>
    case 'alert':
      return <svg {...props} stroke="#f59e0b"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
    default:
      return <svg {...props} stroke="var(--text-muted)"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>
  }
}

/** 心形 SVG 图标（点赞） */
function HeartIcon({ filled, size = 14 }: { filled?: boolean; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24"
      fill={filled ? 'var(--error)' : 'none'}
      stroke={filled ? 'var(--error)' : 'currentColor'}
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
    >
      <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
    </svg>
  )
}

/** 评论气泡 SVG 图标 */
function CommentIcon({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
    >
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
    </svg>
  )
}

/** 空状态 SVG 图标 */
function EmptyIcon({ size = 48 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="var(--text-muted)" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"
      style={{ opacity: 0.5 }}
    >
      <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
      <line x1="8" y1="21" x2="16" y2="21"/>
      <line x1="12" y1="17" x2="12" y2="21"/>
    </svg>
  )
}

/** 广场标题 SVG 图标 */
function PlazaIcon({ size = 24 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="url(#plazaGrad)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"
    >
      <defs>
        <linearGradient id="plazaGrad" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#10b981"/>
          <stop offset="100%" stopColor="#06b6d4"/>
        </linearGradient>
      </defs>
      <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
      <polyline points="9 22 9 12 15 12 15 22"/>
    </svg>
  )
}

/* ---- 样式常量 ---- */

const GLASS_CARD: CSSProperties = {
  background: 'var(--bg-glass)',
  backdropFilter: 'blur(12px)',
  WebkitBackdropFilter: 'blur(12px)',
  border: '1px solid rgba(255,255,255,0.06)',
  borderRadius: 'var(--radius-lg)',
  transition: 'all 0.2s ease',
}

export default function PlazaPage() {
  const { t } = useI18n()
  const [posts, setPosts] = useState<Post[]>([])
  const [agents, setAgents] = useState<{ id: string; name: string }[]>([])
  const [loading, setLoading] = useState(true)
  const [newPost, setNewPost] = useState('')
  const [postAgent, setPostAgent] = useState('')
  const [postType, setPostType] = useState('discovery')
  const [expandedPost, setExpandedPost] = useState<string | null>(null)
  const [comments, setComments] = useState<Record<string, Comment[]>>({})
  const [newComment, setNewComment] = useState('')
  const [hoveredCard, setHoveredCard] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const [p, a] = await Promise.all([
        invoke<Post[]>('plaza_list_posts', { limit: 50 }),
        invoke<any[]>('list_agents'),
      ])
      setPosts(p)
      setAgents(a)
      if (!postAgent && a.length > 0) setPostAgent(a[0].id)
    } catch (e) { console.error(e) }
    setLoading(false)
  }, [])

  useEffect(() => { load() }, [load])

  const handlePost = async () => {
    if (!newPost.trim() || !postAgent) return
    try {
      await invoke('plaza_create_post', { agentId: postAgent, content: newPost.trim(), postType })
      setNewPost('')
      await load()
    } catch (e) { toast.error(String(e)) }
  }

  const handleLike = async (postId: string) => {
    try {
      await invoke('plaza_like_post', { postId })
      setPosts(prev => prev.map(p => p.id === postId ? { ...p, likes: p.likes + 1 } : p))
    } catch (e) { toast.error(String(e)) }
  }

  const loadComments = async (postId: string) => {
    if (expandedPost === postId) { setExpandedPost(null); return }
    try {
      const c = await invoke<Comment[]>('plaza_get_comments', { postId })
      setComments(prev => ({ ...prev, [postId]: c }))
      setExpandedPost(postId)
    } catch (e) { console.error('Load comments failed:', e) }
  }

  const handleComment = async (postId: string) => {
    if (!newComment.trim() || !postAgent) return
    try {
      await invoke('plaza_add_comment', { postId, agentId: postAgent, content: newComment.trim() })
      setNewComment('')
      const c = await invoke<Comment[]>('plaza_get_comments', { postId })
      setComments(prev => ({ ...prev, [postId]: c }))
      setPosts(prev => prev.map(p => p.id === postId ? { ...p, commentCount: p.commentCount + 1 } : p))
    } catch (e) { toast.error(String(e)) }
  }

  if (loading) return <div style={{ padding: 24, color: 'var(--text-muted)' }}>{t('common.loading')}</div>

  return (
    <div className="plaza-container" style={{ padding: '24px 32px', maxWidth: 700 }}>
      {/* 标题区：accent 渐变图标 + 渐变标题 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, margin: '0 0 24px' }}>
        <PlazaIcon size={26} />
        <h1 style={{
          margin: 0, fontSize: 22, fontWeight: 700,
          background: 'var(--accent-gradient)',
          WebkitBackgroundClip: 'text',
          WebkitTextFillColor: 'transparent',
          backgroundClip: 'text',
        }}>
          {t('plaza.title')}
        </h1>
      </div>

      {/* 发帖区：glass 背景卡片 */}
      <div style={{
        ...GLASS_CARD, padding: 16, marginBottom: 24,
      }}>
        <div style={{ display: 'flex', gap: 8, marginBottom: 10, flexWrap: 'wrap' }}>
          <Select value={postAgent} onChange={setPostAgent}
            options={agents.map(a => ({ value: a.id, label: a.name }))}
            style={{ minWidth: 120 }} />
          <Select value={postType} onChange={setPostType}
            options={[
              { value: 'discovery', label: t('plaza.typeDiscovery') },
              { value: 'status', label: t('plaza.typeStatus') },
              { value: 'task_result', label: t('plaza.typeTaskResult') },
              { value: 'reflection', label: t('plaza.typeReflection') },
            ]}
            style={{ minWidth: 140 }} />
        </div>
        <textarea
          value={newPost} onChange={e => setNewPost(e.target.value)}
          placeholder={t('plaza.postPlaceholder')}
          rows={3}
          style={{
            width: '100%', padding: 10, borderRadius: 8,
            border: '1px solid var(--border-subtle)', fontSize: 13, resize: 'vertical',
          }}
        />
        <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 8 }}>
          <button onClick={handlePost} disabled={!newPost.trim()}
            style={{
              padding: '6px 20px', borderRadius: 8, border: 'none',
              background: 'var(--accent-gradient)', color: '#fff', fontSize: 13,
              cursor: 'pointer', fontWeight: 500,
            }}>
            {t('plaza.btnPost')}
          </button>
        </div>
      </div>

      {/* Feed */}
      {posts.length === 0 ? (
        <div style={{
          textAlign: 'center', padding: 60, color: 'var(--text-muted)',
          display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12,
        }}>
          <EmptyIcon size={48} />
          <span style={{ fontSize: 14 }}>{t('plaza.empty')}</span>
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {posts.map(post => {
            const isHovered = hoveredCard === post.id
            return (
              <div key={post.id}
                onMouseEnter={() => setHoveredCard(post.id)}
                onMouseLeave={() => setHoveredCard(null)}
                style={{
                  ...GLASS_CARD, padding: 16,
                  transform: isHovered ? 'translateY(-2px)' : 'none',
                  boxShadow: isHovered ? '0 8px 24px rgba(0,0,0,0.25)' : '0 2px 8px rgba(0,0,0,0.12)',
                  borderColor: isHovered ? 'rgba(255,255,255,0.1)' : 'rgba(255,255,255,0.06)',
                }}
              >
                {/* 头部 */}
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                  <PostTypeIcon type={post.postType} size={20} />
                  <span style={{ fontWeight: 600, fontSize: 14, color: 'var(--text-primary)' }}>{post.agentName}</span>
                  <span style={{
                    fontSize: 10, padding: '1px 6px', borderRadius: 8,
                    backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                  }}>{post.postType}</span>
                  <span style={{ flex: 1 }} />
                  <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                    {new Date(post.createdAt).toLocaleString()}
                  </span>
                </div>

                {/* 内容 */}
                <div style={{ fontSize: 13, lineHeight: 1.6, color: 'var(--text-primary)', whiteSpace: 'pre-wrap' }}>
                  {post.content}
                </div>

                {/* 操作栏 */}
                <div style={{ display: 'flex', gap: 16, marginTop: 10, fontSize: 12, color: 'var(--text-muted)' }}>
                  <button onClick={() => handleLike(post.id)}
                    style={{
                      background: 'none', border: 'none', cursor: 'pointer', fontSize: 12,
                      color: post.likes > 0 ? 'var(--error)' : 'var(--text-muted)',
                      display: 'flex', alignItems: 'center', gap: 4,
                      padding: '4px 8px', borderRadius: 6,
                      transition: 'color 0.15s',
                    }}>
                    <HeartIcon filled={post.likes > 0} size={14} />
                    {post.likes}
                  </button>
                  <button onClick={() => loadComments(post.id)}
                    style={{
                      background: 'none', border: 'none', cursor: 'pointer', fontSize: 12,
                      color: expandedPost === post.id ? 'var(--accent)' : 'var(--text-muted)',
                      display: 'flex', alignItems: 'center', gap: 4,
                      padding: '4px 8px', borderRadius: 6,
                      transition: 'color 0.15s',
                    }}>
                    <CommentIcon size={14} />
                    {post.commentCount}
                  </button>
                </div>

                {/* 评论区：缩进 + 细左边线 */}
                {expandedPost === post.id && (
                  <div style={{ marginTop: 12, paddingTop: 12, borderTop: '1px solid var(--border-subtle)' }}>
                    {(comments[post.id] || []).map(c => (
                      <div key={c.id} style={{
                        marginBottom: 8, fontSize: 12, color: 'var(--text-secondary)',
                        paddingLeft: 12, borderLeft: '2px solid var(--border-subtle)',
                        marginLeft: 4,
                      }}>
                        <span style={{ fontWeight: 600, color: 'var(--text-primary)' }}>{c.agentName}</span>
                        {' '}{c.content}
                        <span style={{ marginLeft: 8, color: 'var(--text-muted)', fontSize: 10 }}>
                          {new Date(c.createdAt).toLocaleTimeString()}
                        </span>
                      </div>
                    ))}
                    <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
                      <input value={newComment} onChange={e => setNewComment(e.target.value)}
                        onKeyDown={e => e.key === 'Enter' && handleComment(post.id)}
                        placeholder={t('plaza.commentPlaceholder')}
                        style={{ flex: 1, padding: '5px 10px', borderRadius: 6, border: '1px solid var(--border-subtle)', fontSize: 12 }}
                      />
                      <button onClick={() => handleComment(post.id)}
                        style={{
                          padding: '5px 12px', borderRadius: 6, border: 'none',
                          background: 'var(--accent-gradient)', color: '#fff', fontSize: 11,
                          cursor: 'pointer', fontWeight: 500,
                        }}>
                        {t('plaza.btnComment')}
                      </button>
                    </div>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
