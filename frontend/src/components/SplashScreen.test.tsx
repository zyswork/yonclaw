import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import SplashScreen from './SplashScreen'

describe('SplashScreen', () => {
  it('应该渲染启动屏幕容器', () => {
    render(<SplashScreen />)
    expect(screen.getByTestId('splash-screen')).toBeInTheDocument()
  })

  it('应该显示加载动画', () => {
    render(<SplashScreen />)
    expect(screen.getByTestId('splash-spinner')).toBeInTheDocument()
  })

  it('应该显示默认状态消息', () => {
    render(<SplashScreen />)
    // i18n: common.loading = '加载中...'
    expect(screen.getByText(/加载中/)).toBeInTheDocument()
  })

  it('应该支持自定义状态消息', () => {
    render(<SplashScreen message="正在连接后端..." />)
    expect(screen.getByText('正在连接后端...')).toBeInTheDocument()
  })

  it('应该支持自定义进度百分比', () => {
    render(<SplashScreen progress={50} />)
    expect(screen.getByTestId('splash-progress')).toHaveAttribute('aria-valuenow', '50')
  })
})
