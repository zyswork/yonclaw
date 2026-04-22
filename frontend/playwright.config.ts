import { defineConfig, devices } from '@playwright/test'

/**
 * Playwright 配置
 *
 * 需先启动 vite dev server：`npx vite --port 5173`
 */
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  fullyParallel: false,
  retries: 0,
  reporter: 'list',
  use: {
    baseURL: process.env.E2E_BASE ?? 'http://localhost:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
  webServer: process.env.CI ? {
    command: 'npx vite --port 5173',
    port: 5173,
    reuseExistingServer: false,
    timeout: 60_000,
  } : undefined,
})
