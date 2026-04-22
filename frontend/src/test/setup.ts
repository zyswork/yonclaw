import '@testing-library/jest-dom'
import { vi } from 'vitest'

// Polyfill scrollIntoView（jsdom 不实现）
Object.defineProperty(Element.prototype, 'scrollIntoView', {
  value: vi.fn(),
  writable: true,
})

// Mock window.matchMedia（jsdom 不支持）
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
})

// 设置测试语言为中文（与大部分测试断言文本一致）
localStorage.setItem('xianzhu.locale', 'zh-CN')

// Mock Tauri IPC（测试环境无 Tauri 运行时）
Object.defineProperty(window, '__TAURI_IPC__', {
  value: vi.fn(),
  writable: true,
})

// Mock @tauri-apps/api/tauri
vi.mock('@tauri-apps/api/tauri', () => ({
  invoke: vi.fn(async (cmd: string) => {
    // 为常见命令返回合理的默认值
    const defaults: Record<string, unknown> = {
      get_providers: [],
      list_agents: [{ id: 'test-agent', name: 'Test Agent', model: 'gpt-4o-mini' }],
      get_setting: null,
      get_settings_by_prefix: {},
      get_user_profile: { nickname: '', bio: '', avatarPath: '' },
      get_user_avatar: null,
      get_api_status: { total_tokens: 0, daily_limit: 0, used_today: 0 },
      get_token_stats: { total_tokens: 0, total_input_tokens: 0, total_output_tokens: 0, total_calls: 0, models: [] },
      get_token_daily_stats: [],
      list_sessions: [],
      load_structured_messages: [],
    }
    return defaults[cmd] ?? null
  }),
}))

// Mock @tauri-apps/api/event
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(),
}))
