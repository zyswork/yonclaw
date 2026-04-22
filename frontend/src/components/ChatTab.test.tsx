/**
 * ChatTab smoke tests
 *
 * 覆盖核心渲染路径 + 最近加的拖拽文档抽取。
 * 3500 行组件不追求行覆盖，重在"能渲染 + 关键入口不爆"。
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/tauri'
import ChatTab from './ChatTab'

const renderChat = (agentId = 'test-agent') =>
  render(<MemoryRouter><ChatTab agentId={agentId} /></MemoryRouter>)

describe('ChatTab — smoke', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('渲染不崩（zero sessions 场景）', async () => {
    renderChat()
    // 等一个 tick 让 useEffect 触发
    await waitFor(() => {
      // 任意 div 存在即可（核心在不抛异常）
      expect(document.querySelector('div')).toBeInTheDocument()
    })
  })

  it('会在挂载时调 list_sessions', async () => {
    renderChat('agent-42')
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        'list_sessions',
        expect.objectContaining({ agentId: 'agent-42' })
      )
    })
  })

  it('切换 agentId 时会重新加载 session', async () => {
    const { rerender } = renderChat('agent-a')
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        'list_sessions',
        expect.objectContaining({ agentId: 'agent-a' })
      )
    })
    // 切 agent
    rerender(<MemoryRouter><ChatTab agentId="agent-b" /></MemoryRouter>)
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        'list_sessions',
        expect.objectContaining({ agentId: 'agent-b' })
      )
    })
  })

  it('list_sessions 返回数据时会渲染会话', async () => {
    const mockInvoke = invoke as unknown as ReturnType<typeof vi.fn>
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_sessions') {
        return [
          { id: 's1', title: '测试会话一', agentId: 'test-agent', createdAt: Date.now(), lastMessageAt: Date.now() },
        ]
      }
      return null
    })
    renderChat()
    await waitFor(() => {
      expect(screen.getByText('测试会话一')).toBeInTheDocument()
    })
  })
})

describe('ChatTab — 拖拽文档自动抽取', () => {
  // jsdom 没 DataTransfer，造个最小 shim
  const mkDataTransfer = (files: File[]) => ({
    files: Object.assign(files, { item: (i: number) => files[i] }),
    types: ['Files'],
    items: files.map(f => ({ kind: 'file', type: f.type, getAsFile: () => f })),
    dropEffect: 'copy',
    effectAllowed: 'all',
  })

  // 提供会话的 mock 让 textarea 渲染出来
  const mockWithSession = () => {
    const mockInvoke = invoke as unknown as ReturnType<typeof vi.fn>
    mockInvoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case 'list_sessions':
          return [{ id: 's1', title: '当前会话', agentId: 'test-agent', createdAt: Date.now(), lastMessageAt: Date.now() }]
        case 'load_structured_messages': return []
        case 'get_session_messages': return []
        case 'list_agents': return [{ id: 'test-agent', name: 'Test', model: 'gpt-4o-mini' }]
        default: return null
      }
    })
    return mockInvoke
  }

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('拖 PDF 文件时调 parse_document', async () => {
    const mockInvoke = mockWithSession()
    const parsedText = '这是 PDF 的正文内容'
    const baseImpl = mockInvoke.getMockImplementation()!
    mockInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'parse_document') return parsedText
      return baseImpl(cmd, args)
    })

    renderChat()
    await waitFor(() => expect(document.querySelector('input[placeholder*="输入"]')).toBeInTheDocument())

    const container = document.querySelector('input[placeholder*="输入"]')!.closest('div[style*="padding"]')!

    const file = new File(['pdf bytes'], 'report.pdf', { type: 'application/pdf' })
    Object.defineProperty(file, 'path', { value: '/tmp/report.pdf', writable: false })

    fireEvent.drop(container, { dataTransfer: mkDataTransfer([file]) })

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        'parse_document',
        expect.objectContaining({ filePath: '/tmp/report.pdf' })
      )
    })
  })

  it('拖 .png 图片时不调 parse_document（走图片附件路径）', async () => {
    const mockInvoke = mockWithSession()
    renderChat()
    await waitFor(() => expect(document.querySelector('input[placeholder*="输入"]')).toBeInTheDocument())

    const container = document.querySelector('input[placeholder*="输入"]')!.closest('div[style*="padding"]')!
    const file = new File(['png bytes'], 'pic.png', { type: 'image/png' })
    fireEvent.drop(container, { dataTransfer: mkDataTransfer([file]) })

    await new Promise(r => setTimeout(r, 50))
    const parseCalls = mockInvoke.mock.calls.filter(c => c[0] === 'parse_document')
    expect(parseCalls.length).toBe(0)
  })

  it('拖 .zip 等非文档/非图片文件时仅插入路径，不调 parse_document', async () => {
    const mockInvoke = mockWithSession()
    renderChat()
    await waitFor(() => expect(document.querySelector('input[placeholder*="输入"]')).toBeInTheDocument())

    const container = document.querySelector('input[placeholder*="输入"]')!.closest('div[style*="padding"]')!
    const file = new File(['zip bytes'], 'archive.zip', { type: 'application/zip' })
    Object.defineProperty(file, 'path', { value: '/tmp/archive.zip', writable: false })

    fireEvent.drop(container, { dataTransfer: mkDataTransfer([file]) })

    await new Promise(r => setTimeout(r, 50))
    const parseCalls = mockInvoke.mock.calls.filter(c => c[0] === 'parse_document')
    expect(parseCalls.length).toBe(0)
  })
})
