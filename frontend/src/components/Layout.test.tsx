import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import Layout from './Layout'

describe('Layout 组件', () => {
  it('应该渲染导航栏（包含 "衔烛" 文本）', () => {
    render(
      <Layout>
        <div>测试内容</div>
      </Layout>
    )
    expect(screen.getByText('衔烛')).toBeInTheDocument()
  })

  it('应该渲染侧边栏（包含 "仪表板" 文本）', () => {
    render(
      <Layout>
        <div>测试内容</div>
      </Layout>
    )
    expect(screen.getByText('仪表板')).toBeInTheDocument()
  })

  it('应该渲染子内容', () => {
    render(
      <Layout>
        <div>测试内容</div>
      </Layout>
    )
    expect(screen.getByText('测试内容')).toBeInTheDocument()
  })
})
