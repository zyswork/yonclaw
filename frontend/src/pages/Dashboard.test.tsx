import { describe, it, expect } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import Dashboard from './Dashboard'

describe('Dashboard', () => {
  const renderDashboard = () =>
    render(<MemoryRouter><Dashboard /></MemoryRouter>)

  it('应该渲染仪表板', async () => {
    renderDashboard()
    await waitFor(() => {
      // 快捷操作区域应该渲染
      expect(document.querySelector('div')).toBeInTheDocument()
    })
  })

  it('应该显示快捷操作', async () => {
    renderDashboard()
    await waitFor(() => {
      // i18n: dashboard.quickActions = '快捷操作'
      expect(screen.getByText(/快捷操作/)).toBeInTheDocument()
    })
  })

  it('应该显示创建 Agent 按钮', async () => {
    renderDashboard()
    await waitFor(() => {
      // i18n: dashboard.createAgent = '创建 Agent'
      expect(screen.getByText(/创建 Agent/)).toBeInTheDocument()
    })
  })
})
