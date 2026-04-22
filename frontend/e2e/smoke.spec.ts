/**
 * E2E Smoke Test — 使用 Playwright + webkit/chromium 直连 Vite dev server
 *
 * 前置：
 *   npm i -D @playwright/test
 *   npx playwright install chromium
 *
 * 运行：
 *   npx vite &  # 启动前端 dev server（仅 UI 层，Tauri API 会失败但 UI 可达）
 *   npx playwright test
 *
 * 注意：此 smoke 测试只验证 UI 层能渲染，Tauri `invoke` 调用会失败是预期（需 stub）。
 * 完整 E2E 需配合 `tauri-driver` + WebDriver，见 https://tauri.app/v1/guides/testing/webdriver/
 */

import { test, expect } from '@playwright/test'

const BASE = process.env.E2E_BASE ?? 'http://localhost:5173'
// 跨平台快捷键：macOS 用 Meta (Cmd)，其他用 Control
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control'

test.describe('Smoke — UI renders', () => {
  test.beforeEach(async ({ page }) => {
    // Stub Tauri invoke：在浏览器里注入假的 __TAURI__ 对象，避免 UI 调用崩溃
    await page.addInitScript(() => {
      ;(window as any).__TAURI__ = {
        invoke: async (cmd: string) => {
          if (cmd === 'list_agents') return []
          if (cmd === 'get_providers') return []
          if (cmd === 'health_check') return { status: 'healthy', db: true, agents: 0, memories: 0, today_tokens: 0, response_cache_entries: 0 }
          if (cmd === 'get_cache_stats') return { response_cache: { entries: 0, total_hits: 0 }, embedding_cache: { entries: 0 } }
          if (cmd === 'get_scheduler_status') return null
          if (cmd === 'list_subagent_runs') return []
          return null
        },
        event: {
          listen: async () => () => {},
        },
      }
    })
    await page.goto(BASE)
  })

  test('loads the app shell', async ({ page }) => {
    await expect(page).toHaveTitle(/XianZhu|衔烛|Claw/i)
  })

  test('Cmd+K opens command palette', async ({ page }) => {
    await page.keyboard.press(`${MOD}+K`)
    // 命令面板应该出现，输入框可聚焦
    await expect(page.getByPlaceholder(/输入页面|Agent 名称|命令/)).toBeVisible({ timeout: 2000 })
  })

  test('Escape closes command palette', async ({ page }) => {
    await page.keyboard.press(`${MOD}+K`)
    await page.keyboard.press('Escape')
    await expect(page.getByPlaceholder(/输入页面|Agent 名称|命令/)).not.toBeVisible({ timeout: 2000 })
  })

  test('offline banner appears when offline', async ({ context, page }) => {
    await context.setOffline(true)
    // offline event 派发后 banner 应显示
    await expect(page.getByText(/网络已断开/)).toBeVisible({ timeout: 2000 })
    await context.setOffline(false)
    // 恢复后短暂显示"已恢复"
    await expect(page.getByText(/网络已恢复/)).toBeVisible({ timeout: 2000 })
  })
})
