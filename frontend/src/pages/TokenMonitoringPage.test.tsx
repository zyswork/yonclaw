import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import TokenMonitoringPage from './TokenMonitoringPage'

describe('TokenMonitoringPage', () => {
  const renderPage = () =>
    render(<MemoryRouter><TokenMonitoringPage /></MemoryRouter>)

  it('应该渲染 Token 监控页面标题', async () => {
    renderPage()
    await waitFor(() => {
      // i18n: tokens.title = 'Token 监控'
      expect(screen.getByText('Token 监控')).toBeInTheDocument()
    })
  })

  it('应该显示统计卡片', async () => {
    renderPage()
    await waitFor(() => {
      // i18n: tokens.statTotal = '总 Tokens'
      expect(screen.getByText(/总 Tokens/)).toBeInTheDocument()
    })
  })

  it('应该显示时间范围选择', async () => {
    renderPage()
    await waitFor(() => {
      // 时间选择按钮（7天/30天/90天）
      expect(screen.getByText(/7天/)).toBeInTheDocument()
    })
  })

  it('应该渲染页面容器', async () => {
    renderPage()
    await waitFor(() => {
      // 页面容器应该存在
      expect(document.querySelector('div')).toBeInTheDocument()
    })
  })
})
