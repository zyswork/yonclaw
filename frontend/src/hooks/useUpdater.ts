/**
 * 自动更新 Hook
 * 启动时检查更新，有新版本弹出通知
 */

import { useEffect, useState } from 'react'

interface UpdateInfo {
  version: string
  notes: string
  date: string
}

export function useUpdater() {
  const [updateAvailable, setUpdateAvailable] = useState<UpdateInfo | null>(null)
  const [updating, setUpdating] = useState(false)

  useEffect(() => {
    // 延迟 10 秒检查，不影响启动速度
    const timer = setTimeout(() => checkForUpdate(), 10000)
    // 之后每 2 小时检查一次
    const interval = setInterval(() => checkForUpdate(), 2 * 60 * 60 * 1000)
    return () => { clearTimeout(timer); clearInterval(interval) }
  }, [])

  async function checkForUpdate() {
    try {
      const { checkUpdate } = await import('@tauri-apps/api/updater')
      const { shouldUpdate, manifest } = await checkUpdate()
      if (shouldUpdate && manifest) {
        setUpdateAvailable({
          version: manifest.version,
          notes: manifest.body || '',
          date: manifest.date || '',
        })
      }
    } catch (e) {
      // 静默失败，不影响使用
      console.debug('Update check failed:', e)
    }
  }

  async function installUpdate() {
    try {
      setUpdating(true)
      const { installUpdate: install } = await import('@tauri-apps/api/updater')
      const { relaunch } = await import('@tauri-apps/api/process')
      await install()
      await relaunch()
    } catch (e) {
      console.warn('Update install failed:', e)
      setUpdating(false)
    }
  }

  function dismissUpdate() {
    setUpdateAvailable(null)
  }

  return { updateAvailable, updating, installUpdate, dismissUpdate, checkForUpdate }
}
