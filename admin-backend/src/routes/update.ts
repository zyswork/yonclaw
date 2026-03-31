/**
 * 自动更新端点
 * Tauri updater 会定期检查此端点获取最新版本信息
 *
 * 更新信息存储在 settings 表 key='latest_release' 中
 * 由 CI (GitHub Actions) 发布时自动更新
 */

import { Router, Request, Response } from 'express'
import { db } from '../db/sqlite.js'

const router = Router()

/**
 * GET /update/check?target=darwin-aarch64&current_version=0.2.0
 *
 * Tauri updater 格式要求：
 * - 有新版：返回 200 + JSON { version, url, signature, notes, pub_date }
 * - 无新版：返回 204 No Content
 */
router.get('/check', (req: Request, res: Response) => {
  try {
    const { target, current_version } = req.query

    // 从 DB 读取最新发布信息
    const row = db.prepare("SELECT value FROM settings WHERE key = 'latest_release'").get() as { value: string } | undefined
    if (!row) {
      res.status(204).send()
      return
    }

    const release = JSON.parse(row.value) as {
      version: string
      notes: string
      pub_date: string
      platforms: Record<string, { url: string; signature: string }>
    }

    // 比较版本号
    if (!isNewerVersion(release.version, current_version as string)) {
      res.status(204).send()
      return
    }

    // 查找对应平台的下载包
    const targetStr = (target as string) || ''
    const platformInfo = release.platforms[targetStr]
    if (!platformInfo) {
      // 尝试模糊匹配
      const fuzzyKey = Object.keys(release.platforms).find(k =>
        targetStr.includes('darwin') && k.includes('darwin') ||
        targetStr.includes('windows') && k.includes('windows') ||
        targetStr.includes('linux') && k.includes('linux')
      )
      if (!fuzzyKey) {
        res.status(204).send()
        return
      }
      const fuzzyInfo = release.platforms[fuzzyKey]
      res.json({
        version: release.version,
        url: fuzzyInfo.url,
        signature: fuzzyInfo.signature,
        notes: release.notes,
        pub_date: release.pub_date,
      })
      return
    }

    res.json({
      version: release.version,
      url: platformInfo.url,
      signature: platformInfo.signature,
      notes: release.notes,
      pub_date: release.pub_date,
    })
  } catch (error) {
    console.error('[更新] 检查更新失败:', error)
    res.status(204).send() // 出错时不阻止用户使用
  }
})

/**
 * POST /update/publish — CI 发布时调用，更新最新版本信息
 * 需要 Authorization: Bearer <PUBLISH_SECRET>
 */
router.post('/publish', (req: Request, res: Response) => {
  const publishSecret = process.env.UPDATE_PUBLISH_SECRET
  if (!publishSecret) {
    res.status(500).json({ error: '服务端未配置 UPDATE_PUBLISH_SECRET' })
    return
  }

  const authHeader = req.headers['authorization'] || ''
  if (authHeader !== `Bearer ${publishSecret}`) {
    res.status(403).json({ error: '未授权' })
    return
  }

  try {
    const { version, notes, pub_date, platforms } = req.body

    if (!version || !platforms) {
      res.status(400).json({ error: '缺少必填字段: version, platforms' })
      return
    }

    const release = JSON.stringify({ version, notes: notes || '', pub_date: pub_date || new Date().toISOString(), platforms })

    db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES ('latest_release', ?)").run(release)

    console.log(`✓ 发布更新: v${version}`)
    res.json({ success: true, version })
  } catch (error) {
    console.error('[更新] 发布失败:', error)
    res.status(500).json({ error: '发布失败' })
  }
})

/** 简单的语义版本比较 */
function isNewerVersion(latest: string, current: string): boolean {
  if (!latest || !current) return false
  const parse = (v: string) => v.replace(/^v/, '').split('.').map(Number)
  const [lMaj, lMin = 0, lPat = 0] = parse(latest)
  const [cMaj, cMin = 0, cPat = 0] = parse(current)
  if (lMaj !== cMaj) return lMaj > cMaj
  if (lMin !== cMin) return lMin > cMin
  return lPat > cPat
}

export default router
