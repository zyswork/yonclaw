import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import Layout from './Layout'

describe('Layout 组件', () => {
  const renderWithRouter = (ui: React.ReactElement) =>
    render(<MemoryRouter>{ui}</MemoryRouter>)

  it('应该渲染子内容', () => {
    renderWithRouter(
      <Layout>
        <div>测试内容</div>
      </Layout>
    )
    expect(screen.getByText('测试内容')).toBeInTheDocument()
  })

  it('应该渲染侧边栏', () => {
    renderWithRouter(
      <Layout>
        <div>测试内容</div>
      </Layout>
    )
    // Sidebar 渲染了导航项
    expect(document.querySelector('.sidebar')).toBeTruthy()
  })
})
