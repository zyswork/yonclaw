// 遥测路由 - 错误报告与设备心跳

import { Router, Request, Response } from 'express'
import { v4 as uuidv4 } from 'uuid'
import { db as sqliteDb } from '../db/sqlite.js'
import { authMiddleware, AuthRequest } from '../middleware/auth.js'

const router = Router()

// 管理员权限检查中间件
const requireAdmin = (req: AuthRequest, res: Response, next: any) => {
  if (!req.user || req.user.role !== 'admin') {
    res.status(403).json({ error: '需要管理员权限' })
    return
  }
  next()
}

// ===== 公开端点（桌面客户端直接调用，无需认证） =====

/**
 * POST /report - 提交错误报告
 */
router.post('/report', (req: Request, res: Response) => {
  try {
    const { userId, deviceId, platform, appVersion, errorType, errorCode, message, context } = req.body

    if (!errorType || !message) {
      res.status(400).json({ error: '缺少必填字段: errorType, message' })
      return
    }

    const id = uuidv4()
    const contextStr = context ? (typeof context === 'string' ? context : JSON.stringify(context)) : null

    const stmt = sqliteDb.prepare(`
      INSERT INTO error_reports (id, userId, deviceId, platform, appVersion, errorType, errorCode, message, context, createdAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(id, userId || null, deviceId || null, platform || null, appVersion || null, errorType, errorCode || null, message, contextStr, new Date().toISOString())

    res.json({ success: true, id })
  } catch (error) {
    console.error('[遥测] 错误报告保存失败:', error)
    res.status(500).json({ error: '保存错误报告失败' })
  }
})

/**
 * POST /heartbeat - 设备心跳上报
 */
router.post('/heartbeat', (req: Request, res: Response) => {
  try {
    const { userId, deviceId, platform, appVersion, agentCount, sessionCount, lastModel } = req.body

    if (!deviceId) {
      res.status(400).json({ error: '缺少必填字段: deviceId' })
      return
    }

    // 获取客户端 IP
    const ip = (req.headers['x-forwarded-for'] as string)?.split(',')[0]?.trim() || req.socket.remoteAddress || ''

    const id = uuidv4()
    const now = new Date().toISOString()

    // UPSERT: 按 deviceId 更新或插入
    const stmt = sqliteDb.prepare(`
      INSERT INTO device_heartbeats (id, userId, deviceId, platform, appVersion, ip, agentCount, sessionCount, lastModel, lastSeen)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT(deviceId) DO UPDATE SET
        userId = excluded.userId,
        platform = excluded.platform,
        appVersion = excluded.appVersion,
        ip = excluded.ip,
        agentCount = excluded.agentCount,
        sessionCount = excluded.sessionCount,
        lastModel = excluded.lastModel,
        lastSeen = excluded.lastSeen
    `)
    stmt.run(id, userId || null, deviceId, platform || null, appVersion || null, ip, agentCount || 0, sessionCount || 0, lastModel || null, now)

    res.json({ success: true })
  } catch (error) {
    console.error('[遥测] 心跳保存失败:', error)
    res.status(500).json({ error: '保存心跳失败' })
  }
})

// ===== 管理端点（需要认证 + 管理员权限） =====

/**
 * GET /errors - 查询错误报告列表
 */
router.get('/errors', authMiddleware, requireAdmin, (req: AuthRequest, res: Response) => {
  try {
    const { userId, errorType, platform, limit: limitStr, offset: offsetStr, startDate, endDate } = req.query
    const limit = parseInt(limitStr as string) || 50
    const offset = parseInt(offsetStr as string) || 0

    let sql = `
      SELECT e.*, u.name as userName, u.email as userEmail
      FROM error_reports e
      LEFT JOIN users u ON e.userId = u.id
      WHERE 1=1
    `
    const params: any[] = []

    if (userId) {
      sql += ' AND e.userId = ?'
      params.push(userId)
    }
    if (errorType) {
      sql += ' AND e.errorType = ?'
      params.push(errorType)
    }
    if (platform) {
      sql += ' AND e.platform = ?'
      params.push(platform)
    }
    if (startDate) {
      sql += ' AND e.createdAt >= ?'
      params.push(startDate)
    }
    if (endDate) {
      sql += ' AND e.createdAt <= ?'
      params.push(endDate)
    }

    // 获取总数
    const countSql = sql.replace(/SELECT e\.\*, u\.name as userName, u\.email as userEmail/, 'SELECT COUNT(*) as total')
    const countResult = sqliteDb.prepare(countSql).get(...params) as any
    const total = countResult?.total || 0

    sql += ' ORDER BY e.createdAt DESC LIMIT ? OFFSET ?'
    params.push(limit, offset)

    const rows = sqliteDb.prepare(sql).all(...params) as any[]

    res.json({
      data: rows.map(row => ({
        ...row,
        context: row.context ? safeJsonParse(row.context) : null,
      })),
      total,
      limit,
      offset,
    })
  } catch (error) {
    console.error('[遥测] 查询错误报告失败:', error)
    res.status(500).json({ error: '查询错误报告失败' })
  }
})

/**
 * GET /errors/stats - 错误统计
 */
router.get('/errors/stats', authMiddleware, requireAdmin, (req: AuthRequest, res: Response) => {
  try {
    const now = new Date()
    const h24 = new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString()
    const d7 = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString()

    // 24 小时总数
    const total24h = (sqliteDb.prepare(
      'SELECT COUNT(*) as cnt FROM error_reports WHERE createdAt >= ?'
    ).get(h24) as any)?.cnt || 0

    // 7 天总数
    const total7d = (sqliteDb.prepare(
      'SELECT COUNT(*) as cnt FROM error_reports WHERE createdAt >= ?'
    ).get(d7) as any)?.cnt || 0

    // 按类型统计（7 天）
    const byTypeRows = sqliteDb.prepare(
      'SELECT errorType, COUNT(*) as cnt FROM error_reports WHERE createdAt >= ? GROUP BY errorType'
    ).all(d7) as any[]
    const byType: Record<string, number> = {}
    for (const row of byTypeRows) {
      byType[row.errorType] = row.cnt
    }

    // 按平台统计（7 天）
    const byPlatformRows = sqliteDb.prepare(
      'SELECT platform, COUNT(*) as cnt FROM error_reports WHERE createdAt >= ? AND platform IS NOT NULL GROUP BY platform'
    ).all(d7) as any[]
    const byPlatform: Record<string, number> = {}
    for (const row of byPlatformRows) {
      byPlatform[row.platform] = row.cnt
    }

    // 按模型统计（从 context JSON 提取 model 字段，7 天）
    const contextRows = sqliteDb.prepare(
      'SELECT context FROM error_reports WHERE createdAt >= ? AND context IS NOT NULL'
    ).all(d7) as any[]
    const byModel: Record<string, number> = {}
    for (const row of contextRows) {
      const ctx = safeJsonParse(row.context)
      if (ctx && ctx.model) {
        byModel[ctx.model] = (byModel[ctx.model] || 0) + 1
      }
    }

    res.json({
      total_24h: total24h,
      total_7d: total7d,
      by_type: byType,
      by_model: byModel,
      by_platform: byPlatform,
    })
  } catch (error) {
    console.error('[遥测] 查询错误统计失败:', error)
    res.status(500).json({ error: '查询错误统计失败' })
  }
})

/**
 * GET /devices - 查询所有设备
 */
router.get('/devices', authMiddleware, requireAdmin, (req: AuthRequest, res: Response) => {
  try {
    const tenMinutesAgo = new Date(Date.now() - 10 * 60 * 1000).toISOString()

    const rows = sqliteDb.prepare(`
      SELECT d.*, u.name as userName, u.email as userEmail,
        CASE WHEN d.lastSeen >= ? THEN 1 ELSE 0 END as isOnline
      FROM device_heartbeats d
      LEFT JOIN users u ON d.userId = u.id OR d.userId = u.email OR d.userId = u.name
      ORDER BY d.lastSeen DESC
    `).all(tenMinutesAgo) as any[]

    res.json({
      data: rows.map(row => ({
        ...row,
        userName: row.userName || row.userId || '--',
        userEmail: row.userEmail || '',
        isOnline: row.isOnline === 1,
      })),
    })
  } catch (error) {
    console.error('[遥测] 查询设备列表失败:', error)
    res.status(500).json({ error: '查询设备列表失败' })
  }
})

/**
 * GET /devices/:userId - 查询指定用户的设备
 */
router.get('/devices/:userId', authMiddleware, (req: AuthRequest, res: Response) => {
  try {
    const { userId } = req.params
    const tenMinutesAgo = new Date(Date.now() - 10 * 60 * 1000).toISOString()

    const rows = sqliteDb.prepare(`
      SELECT d.*,
        CASE WHEN d.lastSeen >= ? THEN 1 ELSE 0 END as isOnline
      FROM device_heartbeats d
      WHERE d.userId = ?
      ORDER BY d.lastSeen DESC
    `).all(tenMinutesAgo, userId) as any[]

    res.json({
      data: rows.map(row => ({
        ...row,
        isOnline: row.isOnline === 1,
      })),
    })
  } catch (error) {
    console.error('[遥测] 查询用户设备失败:', error)
    res.status(500).json({ error: '查询用户设备失败' })
  }
})

// ===== 辅助函数 =====

/**
 * 安全解析 JSON 字符串，失败时返回原始字符串
 */
function safeJsonParse(str: string): any {
  try {
    return JSON.parse(str)
  } catch {
    return str
  }
}

export default router
