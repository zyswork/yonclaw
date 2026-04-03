/**
 * 群聊页面 — 多 Agent 在同一个对话中交互
 *
 * 类似 Telegram 群聊：多个 Agent 作为成员，用户通过 @mention 指定回复的 Agent。
 * 每个 Agent 有独立 session，UI 将所有消息合并为统一时间线。
 */

import { useState, useEffect, useRef, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useI18n } from '../i18n'
import { toast, friendlyError } from '../hooks/useToast'
import Modal from '../components/Modal'
import Select from '../components/Select'

marked.setOptions({ breaks: true, gfm: true })

function renderMd(text: string) {
  const html = marked.parse(text, { async: false }) as string
  const clean = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: ['a','b','blockquote','br','code','del','div','em','h1','h2','h3','h4','hr','i','li','ol','p','pre','span','strong','table','tbody','td','th','thead','tr','ul','img'],
    ALLOWED_ATTR: ['class','href','rel','target','title','src','alt','start'],
  })
  return <div className="markdown-body" dangerouslySetInnerHTML={{ __html: clean }} />
}

// ─── Types ──────────────────────────────────────────────────

interface Agent {
  id: string
  name: string
  model: string
}

interface GroupRoom {
  id: string
  name: string
  memberIds: string[]
  defaultAgentId: string
  freeChat: boolean  // 自由会话模式（所有人发言 vs 仅@触发）
  createdAt: number
}

interface GroupMessage {
  id: string
  sender: 'user' | string  // 'user' 或 agentId
  senderName: string
  content: string
  timestamp: number
  isStreaming?: boolean
}

/** Agent 显示名：name 为空时回退到 model 或 ID 前缀 */
function agentDisplayName(a: Agent): string {
  return a.name || a.model || a.id.slice(0, 8)
}

// Agent 头像颜色
const AGENT_COLORS = [
  '#3b82f6', '#8b5cf6', '#ec4899', '#f59e0b', '#10b981',
  '#06b6d4', '#6366f1', '#f43f5e', '#84cc16', '#14b8a6',
]

function getAgentColor(index: number) {
  return AGENT_COLORS[index % AGENT_COLORS.length]
}

// ─── Main Component ─────────────────────────────────────────

export default function GroupChatPage() {
  const { t } = useI18n()
  const [allAgents, setAllAgents] = useState<Agent[]>([])
  const [rooms, setRooms] = useState<GroupRoom[]>([])
  const [activeRoom, setActiveRoom] = useState<GroupRoom | null>(null)
  const [messages, setMessages] = useState<GroupMessage[]>([])
  const [input, setInput] = useState('')
  const [streaming, setStreaming] = useState(false)
  const [showCreate, setShowCreate] = useState(false)
  const [showMembers, setShowMembers] = useState(false)
  const [mentionDropdown, setMentionDropdown] = useState<{ visible: boolean; filter: string; index: number }>({ visible: false, filter: '', index: 0 })
  const composingRef = useRef(false) // IME 输入法组合中
  const streamBuf = useRef('')
  const streamAgentRef = useRef<string>('')
  const streamingRef = useRef(false)
  const streamSessionRef = useRef<string>('')
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  // 加载所有 Agent + 清理旧的 [group] session 残留
  useEffect(() => {
    invoke<Agent[]>('list_agents').then(agents => {
      setAllAgents(agents)
      // 一次性清理：删除所有 [group] 开头的 session
      agents.forEach(a => {
        invoke<any[]>('list_sessions', { agentId: a.id }).then(sessions => {
          sessions?.filter((s: any) => s.title?.startsWith('[group]')).forEach((s: any) => {
            invoke('delete_session', { sessionId: s.id }).catch(() => {})
          })
        }).catch(() => {})
      })
    }).catch(console.error)
  }, [])

  // 加载群聊列表
  const loadRooms = useCallback(async () => {
    try {
      const raw = await invoke<string | null>('get_setting', { key: 'group_chat_rooms' })
      if (raw) {
        const parsed = JSON.parse(raw) as GroupRoom[]
        setRooms(parsed)
        // 如果有房间且没有选中，选中第一个
        if (parsed.length > 0 && !activeRoom) {
          setActiveRoom(parsed[0])
        }
      }
    } catch (e) { console.error('loadRooms:', e) }
  }, [])

  useEffect(() => { loadRooms() }, [loadRooms])

  // 保存群聊列表
  const saveRooms = async (newRooms: GroupRoom[]) => {
    setRooms(newRooms)
    await invoke('set_setting', { key: 'group_chat_rooms', value: JSON.stringify(newRooms) })
  }

  // 加载群聊消息
  const loadMessages = useCallback(async () => {
    if (!activeRoom) return
    try {
      const raw = await invoke<string | null>('get_setting', { key: `group_messages_${activeRoom.id}` })
      if (raw) {
        try {
          const parsed = JSON.parse(raw) as GroupMessage[]
          // 过滤掉残留的 streaming 消息（异常退出时可能遗留）
          setMessages(parsed.filter(m => !m.isStreaming))
        } catch (parseErr) {
          console.error('群聊消息 JSON 解析失败，重置:', parseErr)
          setMessages([])
        }
      } else {
        setMessages([])
      }
    } catch { setMessages([]) }
  }, [activeRoom?.id])

  useEffect(() => {
    loadMessages()
    // 切换房间时重置 streaming 状态
    streamingRef.current = false
    setStreaming(false)
    streamBuf.current = ''
    streamAgentRef.current = ''
  }, [loadMessages])

  // 保存消息
  const saveMessages = async (msgs: GroupMessage[]) => {
    if (!activeRoom) return
    // 只保存非 streaming 的消息
    const toSave = msgs.filter(m => !m.isStreaming).slice(-200) // 最多保存 200 条
    await invoke('set_setting', {
      key: `group_messages_${activeRoom.id}`,
      value: JSON.stringify(toSave),
    }).catch(console.error)
  }

  // 用 ref 保存 activeRoom，避免闭包陷阱
  const activeRoomRef = useRef(activeRoom)
  useEffect(() => { activeRoomRef.current = activeRoom }, [activeRoom])

  // 直接保存（不依赖闭包中的 activeRoom）
  const saveMessagesNow = useCallback((msgs: GroupMessage[]) => {
    const room = activeRoomRef.current
    if (!room) return
    const toSave = msgs.filter(m => !m.isStreaming).slice(-200)
    invoke('set_setting', {
      key: `group_messages_${room.id}`,
      value: JSON.stringify(toSave),
    }).catch(console.error)
  }, [])

  // 全局监听流式 token
  useEffect(() => {
    const unlisten1 = listen<any>('llm-token', (e) => {
      if (!streamingRef.current) return
      // 解析 payload：新格式 {sessionId, text}，兼容旧格式 string
      const payload = e.payload
      const tokenSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      const tokenText = typeof payload === 'object' ? (payload?.text ?? '') : String(payload ?? '')
      // 跨会话过滤：如果事件携带 sessionId，必须匹配当前流式会话
      if (tokenSessionId && streamSessionRef.current && tokenSessionId !== streamSessionRef.current) return
      // 重试重置信号
      if (tokenText === '\x00__XIANZHU_RETRY__') { streamBuf.current = ''; return }
      streamBuf.current += tokenText
      const agentId = streamAgentRef.current
      setMessages(prev => {
        const copy = [...prev]
        for (let i = copy.length - 1; i >= 0; i--) {
          if (copy[i].isStreaming && copy[i].sender === agentId) {
            copy[i] = { ...copy[i], content: streamBuf.current }
            break
          }
        }
        return copy
      })
    })
    // 也监听 llm-done/llm-error 作为兜底，防止 streaming 状态卡住
    const unlisten2 = listen<any>('llm-done', (e) => {
      const payload = e.payload
      const doneSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      // 跨会话过滤
      if (doneSessionId && streamSessionRef.current && doneSessionId !== streamSessionRef.current) return
      if (streamingRef.current) {
        streamingRef.current = false
        streamSessionRef.current = ''
        setStreaming(false)
      }
    })
    const unlisten3 = listen<any>('llm-error', (e) => {
      const payload = e.payload
      const errSessionId = typeof payload === 'object' ? payload?.sessionId : undefined
      // 跨会话过滤
      if (errSessionId && streamSessionRef.current && errSessionId !== streamSessionRef.current) return
      if (streamingRef.current) {
        streamingRef.current = false
        streamSessionRef.current = ''
        setStreaming(false)
      }
    })

    return () => {
      unlisten1.then(f => f())
      unlisten2.then(f => f())
      unlisten3.then(f => f())
    }
  }, [])

  // 自动滚动到底部
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  // 解析 @mention → 返回 agentId
  const parseMention = (text: string): { agentId: string | null; cleanText: string } => {
    if (!activeRoom) return { agentId: null, cleanText: text }
    const mentionRegex = /@(\S+)/
    const match = text.match(mentionRegex)
    if (match) {
      const mentionName = match[1].toLowerCase()
      const agent = allAgents.find(a =>
        activeRoom.memberIds.includes(a.id) &&
        agentDisplayName(a).toLowerCase().includes(mentionName)
      )
      if (agent) {
        return { agentId: agent.id, cleanText: text }
      }
    }
    return { agentId: null, cleanText: text }
  }

  // 构建群聊上下文（给 LLM 的前缀）
  // 构建群聊上下文
  // round=0 是初轮（回答用户），round>0 是讨论轮（回应其他人）
  const buildGroupContext = (
    currentMsgs: GroupMessage[], roomName: string, memberNames: string[],
    agentName: string, round: number,
  ) => {
    const validMsgs = currentMsgs.filter(m => m.content && !m.content.startsWith('Error:'))
    const recent = validMsgs.slice(-20).map(m =>
      `[${m.senderName}]: ${m.content.slice(0, 400)}`
    ).join('\n')
    const members = memberNames.join('、')

    if (round === 0) {
      // 初轮：回答用户问题
      const hasOtherAgentReply = validMsgs.some(m => m.sender !== 'user' && m.senderName !== agentName)
      if (hasOtherAgentReply) {
        // 其他 Agent 已回答，看到后给出自己的观点
        return `[你是「${agentName}」，正在群聊「${roomName}」中，成员有: ${members}。]\n` +
          `[其他成员已经发表了看法，请看完后给出你自己的观点。可以赞同、补充或提出不同角度，2-4句话，有自己的风格。]\n` +
          `\n[对话记录]\n${recent}\n\n`
      } else {
        // 第一个回答的 Agent
        return `[你是「${agentName}」，正在群聊「${roomName}」中。]\n` +
          `[规则：直接回答用户的问题，表达你自己的观点，2-4句话，简短有力，有个人风格。]\n` +
          (recent ? `\n[历史对话]\n${recent}\n\n` : '\n')
      }
    } else {
      // 讨论轮：看到所有人的发言后回应
      return `[你是「${agentName}」，正在群聊「${roomName}」中，成员有: ${members}。]\n` +
        `[这是第 ${round + 1} 轮讨论。请针对其他成员刚才的发言做出回应——你可以赞同并补充、提出质疑、给出不同角度、或者纠正错误。像真人讨论一样自然，2-3句话即可。不要重复自己或别人说过的内容。如果确实没有新观点要补充了，回复 "[pass]"。]\n` +
        `\n[对话记录]\n${recent}\n\n[请发表你的看法]\n`
    }
  }

  // 发送消息给单个 Agent 并等待完成（返回回复内容）
  // 简单方案：invoke 返回值就是完整回复，llm-token 只负责流式显示
  const sendToAgent = async (agentId: string, agentName: string, messageText: string, contextMsgs: GroupMessage[], round = 0): Promise<string> => {
    const placeholderId = `msg-${Date.now()}-${agentId}`
    setMessages(prev => [...prev, {
      id: placeholderId, sender: agentId, senderName: agentName,
      content: '', timestamp: Date.now(), isStreaming: true,
    }])

    // 设置流式显示的目标 Agent，生成唯一 sessionId 用于 SSE 过滤
    const groupSessionId = `group-${Date.now()}-${agentId.slice(0, 8)}`
    streamBuf.current = ''
    streamAgentRef.current = agentId
    streamSessionRef.current = groupSessionId
    streamingRef.current = true
    setStreaming(true)

    try {
      const memberNames = getMemberAgents().map(a => agentDisplayName(a))
      const context = buildGroupContext(contextMsgs, activeRoom!.name, memberNames, agentName, round)
      console.log(`[GroupChat] sendToAgent: ${agentName} (${agentId.slice(0,8)}), round=${round}`)

      // 群聊用轻量命令：不带 tools，纯对话，速度快
      const invokePromise = invoke<string>('send_chat_only', {
        agentId, sessionId: groupSessionId, message: context + messageText,
      })
      const timeoutPromise = new Promise<string>((_, reject) =>
        setTimeout(() => reject(new Error('Response timeout (30s)')), 30000)
      )
      const result = await Promise.race([invokePromise, timeoutPromise])

      const finalContent = result || streamBuf.current || ''
      console.log(`[GroupChat] ${agentName} replied: ${finalContent.slice(0, 50)}...`)
      streamingRef.current = false
      streamSessionRef.current = ''
      setStreaming(false)
      setMessages(prev => prev.map(m =>
        m.id === placeholderId ? { ...m, isStreaming: false, content: finalContent } : m
      ))
      return finalContent
    } catch (e) {
      const errMsg = `Error: ${e}`
      console.error(`[GroupChat] ${agentName} error:`, e)
      toast.error(t('common.error') + ': ' + String(e))
      streamingRef.current = false
      streamSessionRef.current = ''
      setStreaming(false)
      setMessages(prev => prev.map(m =>
        m.id === placeholderId ? { ...m, isStreaming: false, content: errMsg } : m
      ))
      throw e
    }
  }

  const MAX_DISCUSSION_ROUNDS = 3 // 最多讨论轮次（含初轮）

  // 单个 Agent 发言并记录结果
  const agentSpeak = async (
    memberId: string, userText: string, ctx: GroupMessage[], round: number, replies: GroupMessage[],
  ): Promise<{ replied: boolean; reply: GroupMessage | null }> => {
    const agent = allAgents.find(a => a.id === memberId)
    if (!agent) return { replied: false, reply: null }
    try {
      const content = await sendToAgent(memberId, agentDisplayName(agent), userText, ctx, round)
      const trimmed = content?.trim() || ''
      if (trimmed && !trimmed.startsWith('Error:') && trimmed !== '[pass]') {
        const msg: GroupMessage = {
          id: `ctx-${Date.now()}-${memberId}`,
          sender: memberId,
          senderName: agentDisplayName(agent),
          content,
          timestamp: Date.now(),
        }
        return { replied: true, reply: msg }
      } else {
        // pass / 空回复，移除气泡
        setMessages(prev => prev.filter(m =>
          !(m.sender === memberId && (m.content === '[pass]' || !m.content))
        ))
        return { replied: false, reply: null }
      }
    } catch {
      return { replied: false, reply: null }
    }
  }

  // 执行一轮讨论
  const runDiscussionRound = async (
    userText: string, currentContext: GroupMessage[], round: number,
  ): Promise<{ newContext: GroupMessage[]; activeCount: number }> => {
    const room = activeRoom!
    let activeCount = 0
    const roundReplies: GroupMessage[] = []

    if (round === 0) {
      // ── Round 0：默认 Agent 先回答（破冰），其他 Agent 看到后再回应 ──
      const defaultId = room.defaultAgentId
      const othersIds = room.memberIds.filter(id => id !== defaultId)

      // 1) 默认 Agent 独立回答
      const { replied, reply } = await agentSpeak(defaultId, userText, currentContext, 0, roundReplies)
      if (replied && reply) {
        activeCount++
        roundReplies.push(reply)
      }

      // 2) 其他 Agent 看到默认 Agent 的回复后回应
      for (const memberId of othersIds) {
        const ctxWithPrev = [...currentContext, ...roundReplies]
        const res = await agentSpeak(memberId, userText, ctxWithPrev, 0, roundReplies)
        if (res.replied && res.reply) {
          activeCount++
          roundReplies.push(res.reply)
        }
      }
    } else {
      // ── Round 1+：所有人互相讨论，每人能看到之前所有发言 ──
      for (const memberId of room.memberIds) {
        const ctxWithPrev = [...currentContext, ...roundReplies]
        const res = await agentSpeak(memberId, userText, ctxWithPrev, round, roundReplies)
        if (res.replied && res.reply) {
          activeCount++
          roundReplies.push(res.reply)
        }
      }
    }

    return { newContext: [...currentContext, ...roundReplies], activeCount }
  }

  // 发送消息
  const handleSend = async () => {
    // 用 ref 检查（React state 可能因 batching 滞后）
    if (!input.trim() || streamingRef.current || !activeRoom) return
    const userText = input.trim()
    setInput('')

    // 解析 @mention
    const { agentId: mentionedId } = parseMention(userText)

    // 添加用户消息
    const userMsg: GroupMessage = {
      id: `msg-${Date.now()}-user`,
      sender: 'user',
      senderName: t('groupChat.you'),
      content: userText,
      timestamp: Date.now(),
    }
    setMessages(prev => [...prev, userMsg])

    if (mentionedId) {
      // @mention 模式：只发给指定 Agent
      const targetAgent = allAgents.find(a => a.id === mentionedId)
      try {
        await sendToAgent(mentionedId, targetAgent ? agentDisplayName(targetAgent) : 'Agent', userText, [...messages, userMsg], 0)
      } catch (e) { toast.error(friendlyError(e)) }
    } else if (activeRoom.freeChat !== false) {
      // 自由会话模式：所有成员多轮讨论
      let currentContext = [...messages, userMsg]

      for (let round = 0; round < MAX_DISCUSSION_ROUNDS; round++) {
        const { newContext, activeCount } = await runDiscussionRound(userText, currentContext, round)
        currentContext = newContext
        if (activeCount === 0) break
        if (round > 0 && activeCount <= 1) break
      }
    } else {
      // 非自由模式：仅默认 Agent 回复（需 @mention 才触发其他 Agent）
      const defaultAgent = allAgents.find(a => a.id === activeRoom.defaultAgentId)
      try {
        await sendToAgent(activeRoom.defaultAgentId, defaultAgent ? agentDisplayName(defaultAgent) : 'Agent', userText, [...messages, userMsg], 0)
      } catch (e) { toast.error(friendlyError(e)) }
    }

    // 所有轮次完成后：强制重置 streaming 状态 + 保存
    streamingRef.current = false
    setStreaming(false)

    setMessages(prev => {
      const cleaned = prev
        .filter(m => m.content && m.content.trim() !== '[pass]' && !m.isStreaming)
        .filter(m => m.content || m.sender === 'user')
      saveMessagesNow(cleaned)
      return cleaned
    })
  }

  // @mention 自动补全
  const handleInputChange = (value: string) => {
    setInput(value)
    // 检测 @ 触发
    const lastAt = value.lastIndexOf('@')
    if (lastAt >= 0 && !value.slice(lastAt).includes(' ')) {
      const filter = value.slice(lastAt + 1).toLowerCase()
      setMentionDropdown({ visible: true, filter, index: 0 })
    } else {
      setMentionDropdown({ visible: false, filter: '', index: 0 })
    }
  }

  const filteredMentionAgents = activeRoom
    ? allAgents.filter(a =>
        activeRoom.memberIds.includes(a.id) &&
        agentDisplayName(a).toLowerCase().includes(mentionDropdown.filter)
      )
    : []

  const selectMention = (agent: Agent) => {
    const lastAt = input.lastIndexOf('@')
    const newInput = input.slice(0, lastAt) + `@${agentDisplayName(agent)} `
    setInput(newInput)
    setMentionDropdown({ visible: false, filter: '', index: 0 })
    inputRef.current?.focus()
  }

  // 获取 Agent 在群聊中的成员信息
  const getMemberAgents = () => {
    if (!activeRoom) return []
    return activeRoom.memberIds
      .map(id => allAgents.find(a => a.id === id))
      .filter(Boolean) as Agent[]
  }

  // ─── 创建群聊 ─────────────────────────────────

  const [newGroupName, setNewGroupName] = useState('')
  const [selectedMembers, setSelectedMembers] = useState<Set<string>>(new Set())

  const handleCreateRoom = async () => {
    if (!newGroupName.trim() || selectedMembers.size < 1) return
    const memberIds = Array.from(selectedMembers)
    const newRoom: GroupRoom = {
      id: `group-${Date.now()}`,
      name: newGroupName.trim(),
      memberIds,
      defaultAgentId: memberIds[0],
      freeChat: true,
      createdAt: Date.now(),
    }
    const newRooms = [...rooms, newRoom]
    await saveRooms(newRooms)
    setActiveRoom(newRoom)
    setMessages([])
    setShowCreate(false)
    setNewGroupName('')
    setSelectedMembers(new Set())
  }

  const handleDeleteRoom = async (roomId: string) => {
    const newRooms = rooms.filter(r => r.id !== roomId)
    await saveRooms(newRooms)
    if (activeRoom?.id === roomId) {
      setActiveRoom(newRooms[0] || null)
    }
    // 清理消息
    await invoke('set_setting', { key: `group_messages_${roomId}`, value: '' }).catch(() => {})
  }

  const handleAddMember = async (agentId: string) => {
    if (!activeRoom || activeRoom.memberIds.includes(agentId)) return
    const updated = { ...activeRoom, memberIds: [...activeRoom.memberIds, agentId] }
    setActiveRoom(updated)
    const newRooms = rooms.map(r => r.id === updated.id ? updated : r)
    await saveRooms(newRooms)
  }

  const handleRemoveMember = async (agentId: string) => {
    if (!activeRoom || activeRoom.memberIds.length <= 1) return
    const updated = {
      ...activeRoom,
      memberIds: activeRoom.memberIds.filter(id => id !== agentId),
      defaultAgentId: activeRoom.defaultAgentId === agentId ? activeRoom.memberIds[0] : activeRoom.defaultAgentId,
    }
    setActiveRoom(updated)
    const newRooms = rooms.map(r => r.id === updated.id ? updated : r)
    await saveRooms(newRooms)
  }

  // ─── Render ───────────────────────────────────

  const memberAgents = getMemberAgents()

  return (
    <div style={{ display: 'flex', position: 'absolute', inset: 0 }}>
      {/* 左侧：群聊列表 */}
      <div style={{
        width: 220, minWidth: 220, borderRight: '1px solid var(--border-subtle)',
        display: 'flex', flexDirection: 'column', backgroundColor: 'var(--bg-elevated)',
      }}>
        <div style={{ padding: 12, borderBottom: '1px solid var(--border-subtle)' }}>
          <button onClick={() => setShowCreate(true)} style={{
            width: '100%', padding: '8px', backgroundColor: 'var(--accent)', color: '#fff',
            border: 'none', borderRadius: 6, cursor: 'pointer', fontSize: 13, fontWeight: 500,
          }}>
            + {t('groupChat.newGroup')}
          </button>
        </div>

        <div style={{ flex: 1, overflowY: 'auto' }}>
          {rooms.length === 0 ? (
            <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-muted)', fontSize: 13 }}>
              {t('groupChat.noGroups')}
            </div>
          ) : (
            rooms.map(room => {
              const members = room.memberIds
                .map(id => { const a = allAgents.find(x => x.id === id); return a ? agentDisplayName(a) : '?' })
                .join(', ')
              return (
                <div key={room.id}
                  onClick={() => { setActiveRoom(room); setMessages([]) }}
                  style={{
                    padding: '10px 14px', cursor: 'pointer',
                    borderBottom: '1px solid var(--border-subtle)',
                    backgroundColor: activeRoom?.id === room.id ? 'var(--accent-bg)' : 'transparent',
                    borderLeft: activeRoom?.id === room.id ? '3px solid var(--accent)' : '3px solid transparent',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <span style={{ fontSize: 16 }}>{''}</span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: 14, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {room.name}
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--text-muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {members}
                      </div>
                    </div>
                    <button onClick={(e) => { e.stopPropagation(); handleDeleteRoom(room.id) }}
                      style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', fontSize: 14, padding: '0 4px' }}
                    >×</button>
                  </div>
                </div>
              )
            })
          )}
        </div>
      </div>

      {/* 中间：聊天区 */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        {!activeRoom ? (
          <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-muted)' }}>
            {t('groupChat.selectOrCreate')}
          </div>
        ) : (
          <>
            {/* 顶栏：群名 + 成员头像 */}
            <div style={{
              padding: '10px 16px', borderBottom: '1px solid var(--border-subtle)',
              display: 'flex', alignItems: 'center', gap: 12,
            }}>
              <span style={{ fontSize: 18 }}>{''}</span>
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 16, fontWeight: 600 }}>{activeRoom.name}</div>
                <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                  {memberAgents.map(a => agentDisplayName(a)).join(' · ')} · {t('groupChat.memberCount', { n: memberAgents.length })}
                </div>
              </div>
              {/* 成员头像组 */}
              <div style={{ display: 'flex', gap: -8 }}>
                {memberAgents.slice(0, 5).map((a, i) => (
                  <div key={a.id} title={agentDisplayName(a)} style={{
                    width: 28, height: 28, borderRadius: '50%', border: '2px solid var(--bg-primary)',
                    backgroundColor: getAgentColor(i), color: '#fff',
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    fontSize: 12, fontWeight: 600, marginLeft: i > 0 ? -6 : 0,
                    position: 'relative', zIndex: 5 - i,
                  }}>
                    {agentDisplayName(a).charAt(0).toUpperCase()}
                  </div>
                ))}
                {memberAgents.length > 5 && (
                  <div style={{
                    width: 28, height: 28, borderRadius: '50%', border: '2px solid var(--bg-primary)',
                    backgroundColor: 'var(--bg-glass)', color: 'var(--text-muted)',
                    display: 'flex', alignItems: 'center', justifyContent: 'center',
                    fontSize: 10, marginLeft: -6,
                  }}>
                    +{memberAgents.length - 5}
                  </div>
                )}
              </div>
              {/* 自由会话开关 */}
              <button onClick={async () => {
                const updated = { ...activeRoom, freeChat: !activeRoom.freeChat }
                setActiveRoom(updated)
                const newRooms = rooms.map(r => r.id === updated.id ? updated : r)
                await saveRooms(newRooms)
              }}
                title={activeRoom.freeChat !== false ? t('groupChat.freeChatOn') : t('groupChat.freeChatOff')}
                style={{
                  padding: '4px 10px', fontSize: 11, border: '1px solid var(--border-subtle)',
                  borderRadius: 6, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4,
                  backgroundColor: activeRoom.freeChat !== false ? 'var(--accent-bg)' : 'transparent',
                  color: activeRoom.freeChat !== false ? 'var(--accent)' : 'var(--text-muted)',
                }}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  {activeRoom.freeChat !== false
                    ? <><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></>
                    : <><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></>
                  }
                </svg>
                {activeRoom.freeChat !== false ? t('groupChat.freeChat') : t('groupChat.mentionOnly')}
              </button>
              <button onClick={() => setShowMembers(!showMembers)}
                style={{
                  padding: '4px 10px', fontSize: 12, border: '1px solid var(--border-subtle)',
                  borderRadius: 6, cursor: 'pointer', backgroundColor: showMembers ? 'var(--accent-bg)' : 'transparent',
                  color: 'var(--text-secondary)',
                }}>
                {t('groupChat.members')}
              </button>
            </div>

            {/* 消息区 + 可选成员面板 */}
            <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
              {/* 消息列表 */}
              <div style={{ flex: 1, overflowY: 'auto', padding: 16, minWidth: 0 }}>
                {messages.length === 0 && (
                  <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-muted)', fontSize: 13 }}>
                    {t('groupChat.emptyHint')}
                  </div>
                )}
                {messages.map((msg, i) => {
                  const isUser = msg.sender === 'user'
                  const agentIndex = activeRoom.memberIds.indexOf(msg.sender)
                  const color = isUser ? '#374151' : getAgentColor(agentIndex)

                  return (
                    <div key={msg.id || i} style={{
                      marginBottom: 12,
                      display: 'flex',
                      flexDirection: isUser ? 'row-reverse' : 'row',
                      gap: 8,
                      alignItems: 'flex-start',
                    }}>
                      {/* 头像 */}
                      <div style={{
                        width: 32, height: 32, borderRadius: '50%', flexShrink: 0,
                        backgroundColor: isUser ? 'var(--accent)' : color,
                        color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center',
                        fontSize: 13, fontWeight: 600,
                      }}>
                        {isUser ? 'U' : msg.senderName.charAt(0).toUpperCase()}
                      </div>
                      {/* 消息体 */}
                      <div style={{ maxWidth: '70%', minWidth: 0 }}>
                        {/* 发送者名称 */}
                        <div style={{
                          fontSize: 11, fontWeight: 600, marginBottom: 2,
                          color: isUser ? 'var(--text-muted)' : color,
                          textAlign: isUser ? 'right' : 'left',
                        }}>
                          {msg.senderName}
                          {!isUser && (
                            <span style={{ fontWeight: 400, color: 'var(--text-muted)', marginLeft: 4, fontSize: 10 }}>
                              {allAgents.find(a => a.id === msg.sender)?.model || ''}
                            </span>
                          )}
                        </div>
                        {/* 消息内容 */}
                        <div style={{
                          padding: '10px 14px', borderRadius: 12,
                          backgroundColor: isUser ? 'var(--accent)' : 'var(--bg-glass)',
                          color: isUser ? '#fff' : 'var(--text-primary)',
                          fontSize: 14, lineHeight: 1.6, wordBreak: 'break-word',
                          borderTopRightRadius: isUser ? 4 : 12,
                          borderTopLeftRadius: isUser ? 12 : 4,
                        }}>
                          {msg.isStreaming && !msg.content ? (
                            <span style={{ color: 'var(--text-muted)', fontSize: 13 }}>
                              {t('agentDetail.thinking')}...
                            </span>
                          ) : isUser ? (
                            // 用户消息中高亮 @mention
                            <span>{msg.content.split(/(@\S+)/).map((part, pi) =>
                              part.startsWith('@') ? (
                                <span key={pi} style={{ fontWeight: 700, textDecoration: 'underline' }}>{part}</span>
                              ) : <span key={pi}>{part}</span>
                            )}</span>
                          ) : (
                            renderMd(msg.content)
                          )}
                        </div>
                        {/* 时间 */}
                        <div style={{
                          fontSize: 10, color: 'var(--text-muted)', marginTop: 2,
                          textAlign: isUser ? 'right' : 'left',
                        }}>
                          {new Date(msg.timestamp).toLocaleTimeString()}
                        </div>
                      </div>
                    </div>
                  )
                })}
                <div ref={messagesEndRef} />
              </div>

              {/* 右侧成员面板 */}
              {showMembers && (
                <div style={{
                  width: 240, borderLeft: '1px solid var(--border-subtle)',
                  padding: 12, overflowY: 'auto', flexShrink: 0,
                }}>
                  <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 12 }}>{t('groupChat.members')}</div>

                  {/* 当前成员 */}
                  {memberAgents.map((a, i) => (
                    <div key={a.id} style={{
                      display: 'flex', alignItems: 'center', gap: 8, padding: '6px 0',
                      borderBottom: '1px solid var(--border-subtle)',
                    }}>
                      <div style={{
                        width: 24, height: 24, borderRadius: '50%',
                        backgroundColor: getAgentColor(i), color: '#fff',
                        display: 'flex', alignItems: 'center', justifyContent: 'center',
                        fontSize: 11, fontWeight: 600,
                      }}>
                        {agentDisplayName(a).charAt(0).toUpperCase()}
                      </div>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ fontSize: 13, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {agentDisplayName(a)}
                          {a.id === activeRoom.defaultAgentId && (
                            <span style={{ fontSize: 10, color: 'var(--accent)', marginLeft: 4 }}>{t('groupChat.default')}</span>
                          )}
                        </div>
                        <div style={{ fontSize: 10, color: 'var(--text-muted)' }}>{a.model}</div>
                      </div>
                      <button onClick={() => handleRemoveMember(a.id)}
                        style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-muted)', fontSize: 12 }}>
                        {'\u2716'}
                      </button>
                    </div>
                  ))}

                  {/* 添加成员 */}
                  <div style={{ marginTop: 12, fontSize: 12, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6 }}>
                    {t('groupChat.addMember')}
                  </div>
                  {allAgents.filter(a => !activeRoom.memberIds.includes(a.id)).map(a => (
                    <div key={a.id} style={{
                      display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0', cursor: 'pointer',
                    }}
                      onClick={() => handleAddMember(a.id)}
                    >
                      <span style={{ fontSize: 14 }}>{''}</span>
                      <span style={{ flex: 1, fontSize: 12 }}>{agentDisplayName(a)}</span>
                      <span style={{ fontSize: 14, color: 'var(--accent)' }}>+</span>
                    </div>
                  ))}

                  {/* 设置默认 Agent */}
                  <div style={{ marginTop: 16, fontSize: 12, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 6 }}>
                    {t('groupChat.defaultAgent')}
                  </div>
                  <Select
                    value={activeRoom.defaultAgentId}
                    onChange={async (v) => {
                      const updated = { ...activeRoom, defaultAgentId: v }
                      setActiveRoom(updated)
                      const newRooms = rooms.map(r => r.id === updated.id ? updated : r)
                      await saveRooms(newRooms)
                    }}
                    options={memberAgents.map(a => ({ value: a.id, label: agentDisplayName(a) }))}
                    style={{ width: '100%' }}
                  />
                  <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
                    {t('groupChat.defaultAgentHint')}
                  </div>
                </div>
              )}
            </div>

            {/* @mention 补全下拉 */}
            {mentionDropdown.visible && filteredMentionAgents.length > 0 && (
              <div style={{
                padding: '6px 0', borderTop: '1px solid var(--border-subtle)',
                backgroundColor: 'var(--bg-glass)', maxHeight: 150, overflowY: 'auto',
              }}>
                {filteredMentionAgents.map((a, i) => (
                  <div key={a.id}
                    onClick={() => selectMention(a)}
                    style={{
                      padding: '6px 16px', cursor: 'pointer', fontSize: 13,
                      display: 'flex', alignItems: 'center', gap: 8,
                      backgroundColor: i === mentionDropdown.index ? 'var(--accent-bg)' : 'transparent',
                    }}
                    onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = 'var(--accent-bg)' }}
                    onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.backgroundColor = 'transparent' }}
                  >
                    <div style={{
                      width: 20, height: 20, borderRadius: '50%',
                      backgroundColor: getAgentColor(activeRoom.memberIds.indexOf(a.id)),
                      color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center',
                      fontSize: 10, fontWeight: 600,
                    }}>
                      {agentDisplayName(a).charAt(0).toUpperCase()}
                    </div>
                    <span style={{ fontWeight: 500 }}>@{agentDisplayName(a)}</span>
                    <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{a.model}</span>
                  </div>
                ))}
              </div>
            )}

            {/* 输入区 */}
            <div style={{ padding: '10px 16px', borderTop: '1px solid var(--border-subtle)' }}>
              {/* 提示 */}
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 4 }}>
                {activeRoom.freeChat !== false
                  ? t('groupChat.discussHint', { count: memberAgents.length })
                  : t('groupChat.mentionHint', { name: allAgents.find(a => a.id === activeRoom.defaultAgentId)?.name || '?' })
                }
              </div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'flex-end' }}>
                <textarea
                  ref={inputRef as any}
                  value={input}
                  onChange={(e) => handleInputChange(e.target.value)}
                  onCompositionStart={() => { composingRef.current = true }}
                  onCompositionEnd={() => { composingRef.current = false }}
                  onKeyDown={(e) => {
                    if (mentionDropdown.visible && filteredMentionAgents.length > 0) {
                      if (e.key === 'Tab' || (e.key === 'Enter' && !composingRef.current)) {
                        e.preventDefault()
                        selectMention(filteredMentionAgents[mentionDropdown.index])
                        return
                      }
                      if (e.key === 'ArrowDown') {
                        e.preventDefault()
                        setMentionDropdown(prev => ({ ...prev, index: Math.min(prev.index + 1, filteredMentionAgents.length - 1) }))
                        return
                      }
                      if (e.key === 'ArrowUp') {
                        e.preventDefault()
                        setMentionDropdown(prev => ({ ...prev, index: Math.max(prev.index - 1, 0) }))
                        return
                      }
                      if (e.key === 'Escape') {
                        setMentionDropdown({ visible: false, filter: '', index: 0 })
                        return
                      }
                    }
                    // Cmd/Ctrl+Enter 发送，普通 Enter 换行
                    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !composingRef.current) {
                      e.preventDefault()
                      handleSend()
                    }
                  }}
                  placeholder={t('groupChat.inputHint')}
                  disabled={streaming}
                  rows={2}
                  style={{
                    flex: 1, padding: '10px 14px', border: '1px solid var(--border-subtle)',
                    borderRadius: 8, fontSize: 14, resize: 'none', lineHeight: 1.5,
                    backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
                    fontFamily: 'inherit',
                  }}
                />
                <button
                  onClick={handleSend}
                  disabled={streaming || !input.trim()}
                  style={{
                    padding: '10px 20px', backgroundColor: 'var(--accent)', color: '#fff',
                    border: 'none', borderRadius: 8, cursor: streaming || !input.trim() ? 'not-allowed' : 'pointer',
                    opacity: streaming || !input.trim() ? 0.6 : 1, fontSize: 14,
                  }}
                >
                  {streaming ? t('agentDetail.generating') : t('common.send')}
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* 创建群聊弹窗 */}
      <Modal open={showCreate} onClose={() => setShowCreate(false)} title={t('groupChat.createTitle')} footer={
        <>
          <button onClick={() => setShowCreate(false)}
            style={{ padding: '8px 16px', borderRadius: 6, border: '1px solid var(--border-subtle)', cursor: 'pointer', backgroundColor: 'transparent', fontSize: 13, color: 'var(--text-secondary)' }}>
            {t('common.cancel')}
          </button>
          <button onClick={handleCreateRoom}
            disabled={!newGroupName.trim() || selectedMembers.size < 1}
            style={{
              padding: '8px 20px', borderRadius: 6, border: 'none',
              backgroundColor: 'var(--accent)', color: '#fff', cursor: 'pointer', fontSize: 13,
              opacity: (!newGroupName.trim() || selectedMembers.size < 1) ? 0.5 : 1,
            }}>
            {t('groupChat.create')}
          </button>
        </>
      }>
        <label style={{ fontSize: 13, fontWeight: 500, display: 'block', marginBottom: 4, color: 'var(--text-primary)' }}>
          {t('groupChat.groupName')}
        </label>
        <input value={newGroupName} onChange={e => setNewGroupName(e.target.value)}
          placeholder={t('groupChat.groupNameHint')}
          style={{
            width: '100%', padding: '8px 12px', borderRadius: 6,
            border: '1px solid var(--border-subtle)', fontSize: 14, marginBottom: 16,
            boxSizing: 'border-box',
            backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)',
          }}
        />

        <label style={{ fontSize: 13, fontWeight: 500, display: 'block', marginBottom: 8, color: 'var(--text-primary)' }}>
          {t('groupChat.selectMembers')} ({selectedMembers.size})
        </label>
        <div style={{ maxHeight: 300, overflowY: 'auto', marginBottom: 16 }}>
          {allAgents.map((a, i) => {
            const selected = selectedMembers.has(a.id)
            return (
              <div key={a.id}
                onClick={() => {
                  setSelectedMembers(prev => {
                    const next = new Set(prev)
                    if (next.has(a.id)) next.delete(a.id); else next.add(a.id)
                    return next
                  })
                }}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10, padding: '8px 10px',
                  borderRadius: 6, cursor: 'pointer', marginBottom: 4,
                  backgroundColor: selected ? 'var(--accent-bg)' : 'var(--bg-primary)',
                  border: selected ? '1px solid var(--accent)' : '1px solid var(--border-subtle)',
                }}
              >
                <div style={{
                  width: 28, height: 28, borderRadius: '50%',
                  backgroundColor: getAgentColor(i), color: '#fff',
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  fontSize: 12, fontWeight: 600,
                }}>
                  {agentDisplayName(a).charAt(0).toUpperCase()}
                </div>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 14, fontWeight: 500, color: 'var(--text-primary)' }}>{agentDisplayName(a)}</div>
                  <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>{a.model}</div>
                </div>
                <div style={{
                  width: 20, height: 20, borderRadius: 4,
                  border: selected ? 'none' : '2px solid var(--border-subtle)',
                  backgroundColor: selected ? 'var(--accent)' : 'transparent',
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  color: '#fff', fontSize: 14,
                }}>
                  {selected && '\u2713'}
                </div>
              </div>
            )
          })}
        </div>
      </Modal>
    </div>
  )
}
