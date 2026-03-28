// 数据库模块 - SQLite 实现

import { Enterprise } from '../models/enterprise.js'
import { User } from '../models/user.js'
import { KnowledgeBaseDocument } from '../models/knowledge-base.js'
import { AgentTemplate } from '../models/agent-template.js'
import { TokenUsage, TokenQuota, TokenAlert } from '../models/token-usage.js'
import { WebSocketSession } from '../models/session.js'
import sqliteDb from './sqlite.js'
import { initializeDatabase } from './sqlite.js'
import { CacheManager } from '../cache/cache-manager.js'
import { HotColdStrategy } from '../cache/hot-cold-strategy.js'

class Database {
  private cacheManager: CacheManager<any>
  private hotColdStrategy: HotColdStrategy

  constructor() {
    initializeDatabase()

    // 初始化缓存管理器和热冷分层策略
    this.cacheManager = new CacheManager({ maxSize: 1000, ttlMs: 0 })
    this.hotColdStrategy = new HotColdStrategy(this.cacheManager, {
      hotAccessThreshold: 5,
      coldAccessTimeout: 30 * 60 * 1000, // 30 分钟
      checkIntervalMs: 60 * 1000, // 1 分钟
    })
  }

  /**
   * 获取缓存管理器
   */
  getCacheManager(): CacheManager<any> {
    return this.cacheManager
  }

  /**
   * 获取热冷分层策略
   */
  getHotColdStrategy(): HotColdStrategy {
    return this.hotColdStrategy
  }

  // ===== 企业相关方法 =====
  getEnterprises(): Enterprise[] {
    const stmt = sqliteDb.prepare('SELECT * FROM enterprises')
    const rows = stmt.all() as any[]
    return rows.map(row => ({
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }))
  }

  getEnterpriseById(id: string): Enterprise | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM enterprises WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined
    return {
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  createEnterprise(enterprise: Enterprise): Enterprise {
    const stmt = sqliteDb.prepare(`
      INSERT INTO enterprises (id, name, description, logo, website, industry, size, status, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      enterprise.id,
      enterprise.name,
      enterprise.description,
      enterprise.logo,
      enterprise.website,
      enterprise.industry,
      enterprise.size,
      enterprise.status,
      enterprise.createdAt.toISOString(),
      enterprise.updatedAt.toISOString()
    )
    return enterprise
  }

  updateEnterprise(id: string, updates: Partial<Enterprise>): Enterprise | undefined {
    const enterprise = this.getEnterpriseById(id)
    if (!enterprise) return undefined

    const updated = { ...enterprise, ...updates, updatedAt: new Date() }
    const stmt = sqliteDb.prepare(`
      UPDATE enterprises
      SET name = ?, description = ?, logo = ?, website = ?, industry = ?, size = ?, status = ?, updatedAt = ?
      WHERE id = ?
    `)
    stmt.run(
      updated.name,
      updated.description,
      updated.logo,
      updated.website,
      updated.industry,
      updated.size,
      updated.status,
      updated.updatedAt.toISOString(),
      id
    )
    return updated
  }

  deleteEnterprise(id: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM enterprises WHERE id = ?')
    const result = stmt.run(id)
    return result.changes > 0
  }

  // ===== 用户相关方法 =====
  getUsersByEnterpriseId(enterpriseId: string): User[] {
    const stmt = sqliteDb.prepare('SELECT * FROM users WHERE enterpriseId = ?')
    const rows = stmt.all(enterpriseId) as any[]
    return rows.map(row => ({
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }))
  }

  getUserByEmailAndEnterprise(email: string, enterpriseId: string): User | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM users WHERE email = ? AND enterpriseId = ?')
    const row = stmt.get(email, enterpriseId) as any
    if (!row) return undefined
    return {
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  /** 仅按邮箱查找用户（用于验证码登录） */
  getUserByEmail(email: string): User | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM users WHERE email = ?')
    const row = stmt.get(email) as any
    if (!row) return undefined
    return {
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  getUserById(id: string): User | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM users WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined
    return {
      ...row,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  createUser(user: User): User {
    const stmt = sqliteDb.prepare(`
      INSERT INTO users (id, enterpriseId, email, name, passwordHash, role, status, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      user.id,
      user.enterpriseId,
      user.email,
      user.name,
      user.passwordHash || null,
      user.role,
      user.status,
      user.createdAt.toISOString(),
      user.updatedAt.toISOString()
    )
    return user
  }

  updateUser(id: string, updates: Partial<User>): User | undefined {
    const user = this.getUserById(id)
    if (!user) return undefined

    const updated = { ...user, ...updates, updatedAt: new Date() }
    const stmt = sqliteDb.prepare(`
      UPDATE users
      SET email = ?, name = ?, role = ?, status = ?, updatedAt = ?
      WHERE id = ?
    `)
    stmt.run(updated.email, updated.name, updated.role, updated.status, updated.updatedAt.toISOString(), id)
    return updated
  }

  deleteUser(id: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM users WHERE id = ?')
    const result = stmt.run(id)
    return result.changes > 0
  }

  // ===== 用户状态历史相关方法 =====
  createStatusHistory(history: any): any {
    const stmt = sqliteDb.prepare(`
      INSERT INTO user_status_history (id, userId, oldStatus, newStatus, reason, changedBy, createdAt)
      VALUES (?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      history.id,
      history.userId,
      history.oldStatus,
      history.newStatus,
      history.reason || null,
      history.changedBy,
      history.createdAt.toISOString()
    )
    return history
  }

  getStatusHistoryByUserId(userId: string): any[] {
    const stmt = sqliteDb.prepare('SELECT * FROM user_status_history WHERE userId = ? ORDER BY createdAt DESC')
    const rows = stmt.all(userId) as any[]
    return rows.map(row => ({
      ...row,
      createdAt: new Date(row.createdAt),
    }))
  }

  // ===== 知识库文档相关方法 =====
  getDocumentsByEnterpriseId(enterpriseId: string): KnowledgeBaseDocument[] {
    const stmt = sqliteDb.prepare('SELECT * FROM knowledge_base_documents WHERE enterpriseId = ?')
    const rows = stmt.all(enterpriseId) as any[]
    return rows.map(row => this.parseDocument(row))
  }

  getDocumentById(id: string): KnowledgeBaseDocument | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM knowledge_base_documents WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined
    return this.parseDocument(row)
  }

  createDocument(document: KnowledgeBaseDocument): KnowledgeBaseDocument {
    const stmt = sqliteDb.prepare(`
      INSERT INTO knowledge_base_documents
      (id, enterpriseId, title, content, contentType, tags, permissions, version, status, createdBy, createdAt, updatedAt, vectorized)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      document.id,
      document.enterpriseId,
      document.title,
      document.content,
      document.contentType,
      JSON.stringify(document.tags),
      JSON.stringify(document.permissions),
      document.version,
      document.status,
      document.createdBy,
      document.createdAt.toISOString(),
      document.updatedAt.toISOString(),
      document.vectorized ? 1 : 0
    )
    return document
  }

  updateDocument(id: string, updates: Partial<KnowledgeBaseDocument>): KnowledgeBaseDocument | undefined {
    const document = this.getDocumentById(id)
    if (!document) return undefined

    const updated = { ...document, ...updates, updatedAt: new Date() }
    const stmt = sqliteDb.prepare(`
      UPDATE knowledge_base_documents
      SET title = ?, content = ?, contentType = ?, tags = ?, permissions = ?, version = ?, status = ?, updatedAt = ?, vectorized = ?
      WHERE id = ?
    `)
    stmt.run(
      updated.title,
      updated.content,
      updated.contentType,
      JSON.stringify(updated.tags),
      JSON.stringify(updated.permissions),
      updated.version,
      updated.status,
      updated.updatedAt.toISOString(),
      updated.vectorized ? 1 : 0,
      id
    )
    return updated
  }

  deleteDocument(id: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM knowledge_base_documents WHERE id = ?')
    const result = stmt.run(id)
    return result.changes > 0
  }

  searchDocuments(enterpriseId: string, query: string): KnowledgeBaseDocument[] {
    const stmt = sqliteDb.prepare(`
      SELECT * FROM knowledge_base_documents
      WHERE enterpriseId = ? AND (title LIKE ? OR content LIKE ?)
    `)
    const searchTerm = `%${query}%`
    const rows = stmt.all(enterpriseId, searchTerm, searchTerm) as any[]
    return rows.map(row => this.parseDocument(row))
  }

  private parseDocument(row: any): KnowledgeBaseDocument {
    return {
      ...row,
      tags: JSON.parse(row.tags || '[]'),
      permissions: JSON.parse(row.permissions || '{"read":[],"write":[]}'),
      vectorized: row.vectorized === 1,
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  // ===== Agent 模板相关方法 =====
  getTemplatesByEnterpriseId(enterpriseId: string): AgentTemplate[] {
    const stmt = sqliteDb.prepare('SELECT * FROM agent_templates WHERE enterpriseId = ?')
    const rows = stmt.all(enterpriseId) as any[]
    return rows.map(row => this.parseTemplate(row))
  }

  getTemplateById(id: string): AgentTemplate | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM agent_templates WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined
    return this.parseTemplate(row)
  }

  createTemplate(template: AgentTemplate): AgentTemplate {
    const stmt = sqliteDb.prepare(`
      INSERT INTO agent_templates
      (id, enterpriseId, name, description, category, config, version, status, permissions, createdBy, createdAt, updatedAt, publishedAt, tags)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      template.id,
      template.enterpriseId,
      template.name,
      template.description,
      template.category,
      JSON.stringify(template.config),
      template.version,
      template.status,
      JSON.stringify(template.permissions),
      template.createdBy,
      template.createdAt.toISOString(),
      template.updatedAt.toISOString(),
      template.publishedAt?.toISOString() || null,
      JSON.stringify(template.tags)
    )
    return template
  }

  updateTemplate(id: string, updates: Partial<AgentTemplate>): AgentTemplate | undefined {
    const template = this.getTemplateById(id)
    if (!template) return undefined

    const updated = { ...template, ...updates, updatedAt: new Date() }
    const stmt = sqliteDb.prepare(`
      UPDATE agent_templates
      SET name = ?, description = ?, category = ?, config = ?, version = ?, status = ?, permissions = ?, updatedAt = ?, publishedAt = ?, tags = ?
      WHERE id = ?
    `)
    stmt.run(
      updated.name,
      updated.description,
      updated.category,
      JSON.stringify(updated.config),
      updated.version,
      updated.status,
      JSON.stringify(updated.permissions),
      updated.updatedAt.toISOString(),
      updated.publishedAt?.toISOString() || null,
      JSON.stringify(updated.tags),
      id
    )
    return updated
  }

  deleteTemplate(id: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM agent_templates WHERE id = ?')
    const result = stmt.run(id)
    return result.changes > 0
  }

  private parseTemplate(row: any): AgentTemplate {
    return {
      ...row,
      config: JSON.parse(row.config || '{}'),
      permissions: JSON.parse(row.permissions || '{"read":[],"write":[]}'),
      tags: JSON.parse(row.tags || '[]'),
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
      publishedAt: row.publishedAt ? new Date(row.publishedAt) : undefined,
    }
  }

  // ===== Token 使用相关方法 =====
  recordTokenUsage(usage: TokenUsage): TokenUsage {
    const stmt = sqliteDb.prepare(`
      INSERT INTO token_usage (id, enterpriseId, userId, model, inputTokens, outputTokens, totalTokens, cost, timestamp, requestId)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      usage.id,
      usage.enterpriseId,
      usage.userId,
      usage.model,
      usage.inputTokens,
      usage.outputTokens,
      usage.totalTokens,
      usage.cost,
      usage.timestamp.toISOString(),
      usage.requestId
    )
    return usage
  }

  getTokenUsageByEnterpriseId(enterpriseId: string): TokenUsage[] {
    const stmt = sqliteDb.prepare('SELECT * FROM token_usage WHERE enterpriseId = ?')
    const rows = stmt.all(enterpriseId) as any[]
    return rows.map(row => ({
      ...row,
      timestamp: new Date(row.timestamp),
    }))
  }

  // ===== Token 配额相关方法 =====
  getTokenQuotaByEnterpriseId(enterpriseId: string): TokenQuota | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM token_quotas WHERE enterpriseId = ?')
    const row = stmt.get(enterpriseId) as any
    if (!row) return undefined
    return {
      ...row,
      resetDate: new Date(row.resetDate),
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
  }

  createTokenQuota(quota: TokenQuota): TokenQuota {
    const stmt = sqliteDb.prepare(`
      INSERT INTO token_quotas (id, enterpriseId, monthlyLimit, dailyLimit, currentMonthUsage, currentDayUsage, resetDate, status, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      quota.id,
      quota.enterpriseId,
      quota.monthlyLimit,
      quota.dailyLimit,
      quota.currentMonthUsage,
      quota.currentDayUsage,
      quota.resetDate.toISOString(),
      quota.status,
      quota.createdAt.toISOString(),
      quota.updatedAt.toISOString()
    )
    return quota
  }

  updateTokenQuota(id: string, updates: Partial<TokenQuota>): TokenQuota | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM token_quotas WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined

    const quota = {
      ...row,
      resetDate: new Date(row.resetDate),
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
    const updated = { ...quota, ...updates, updatedAt: new Date() }

    const updateStmt = sqliteDb.prepare(`
      UPDATE token_quotas
      SET monthlyLimit = ?, dailyLimit = ?, currentMonthUsage = ?, currentDayUsage = ?, resetDate = ?, status = ?, updatedAt = ?
      WHERE id = ?
    `)
    updateStmt.run(
      updated.monthlyLimit,
      updated.dailyLimit,
      updated.currentMonthUsage,
      updated.currentDayUsage,
      updated.resetDate.toISOString(),
      updated.status,
      updated.updatedAt.toISOString(),
      id
    )
    return updated
  }

  // ===== Token 告警相关方法 =====
  getTokenAlertsByEnterpriseId(enterpriseId: string): TokenAlert[] {
    const stmt = sqliteDb.prepare('SELECT * FROM token_alerts WHERE enterpriseId = ?')
    const rows = stmt.all(enterpriseId) as any[]
    return rows.map(row => ({
      ...row,
      enabled: row.enabled === 1,
      notificationChannels: JSON.parse(row.notificationChannels || '[]'),
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }))
  }

  createTokenAlert(alert: TokenAlert): TokenAlert {
    const stmt = sqliteDb.prepare(`
      INSERT INTO token_alerts (id, enterpriseId, type, threshold, enabled, notificationChannels, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `)
    stmt.run(
      alert.id,
      alert.enterpriseId,
      alert.type,
      alert.threshold,
      alert.enabled ? 1 : 0,
      JSON.stringify(alert.notificationChannels),
      alert.createdAt.toISOString(),
      alert.updatedAt.toISOString()
    )
    return alert
  }

  updateTokenAlert(id: string, updates: Partial<TokenAlert>): TokenAlert | undefined {
    const stmt = sqliteDb.prepare('SELECT * FROM token_alerts WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return undefined

    const alert = {
      ...row,
      enabled: row.enabled === 1,
      notificationChannels: JSON.parse(row.notificationChannels || '[]'),
      createdAt: new Date(row.createdAt),
      updatedAt: new Date(row.updatedAt),
    }
    const updated = { ...alert, ...updates, updatedAt: new Date() }

    const updateStmt = sqliteDb.prepare(`
      UPDATE token_alerts
      SET type = ?, threshold = ?, enabled = ?, notificationChannels = ?, updatedAt = ?
      WHERE id = ?
    `)
    updateStmt.run(
      updated.type,
      updated.threshold,
      updated.enabled ? 1 : 0,
      JSON.stringify(updated.notificationChannels),
      updated.updatedAt.toISOString(),
      id
    )
    return updated
  }

  // ===== WebSocket 会话相关方法 =====
  createWebSocketSession(session: WebSocketSession): boolean {
    const stmt = sqliteDb.prepare(`
      INSERT INTO websocket_sessions (id, userId, connectedAt, lastHeartbeat, status, createdAt, updatedAt)
      VALUES (?, ?, ?, ?, ?, ?, ?)
    `)
    const result = stmt.run(
      session.id,
      session.userId,
      session.connectedAt,
      session.lastHeartbeat,
      session.status,
      new Date().toISOString(),
      new Date().toISOString()
    )
    return result.changes > 0
  }

  getWebSocketSessionById(id: string): WebSocketSession | null {
    const stmt = sqliteDb.prepare('SELECT * FROM websocket_sessions WHERE id = ?')
    const row = stmt.get(id) as any
    if (!row) return null
    return {
      id: row.id,
      userId: row.userId,
      connectedAt: row.connectedAt,
      lastHeartbeat: row.lastHeartbeat,
      status: row.status,
    }
  }

  getWebSocketSessionsByUserId(userId: string): WebSocketSession[] {
    const stmt = sqliteDb.prepare('SELECT * FROM websocket_sessions WHERE userId = ? ORDER BY connectedAt DESC')
    const rows = stmt.all(userId) as any[]
    return rows.map(row => ({
      id: row.id,
      userId: row.userId,
      connectedAt: row.connectedAt,
      lastHeartbeat: row.lastHeartbeat,
      status: row.status,
    }))
  }

  updateWebSocketSessionStatus(id: string, status: string): boolean {
    const stmt = sqliteDb.prepare(`
      UPDATE websocket_sessions
      SET status = ?, updatedAt = ?
      WHERE id = ?
    `)
    const result = stmt.run(status, new Date().toISOString(), id)
    return result.changes > 0
  }

  updateWebSocketSessionHeartbeat(id: string): boolean {
    const stmt = sqliteDb.prepare(`
      UPDATE websocket_sessions
      SET lastHeartbeat = ?, updatedAt = ?
      WHERE id = ?
    `)
    const result = stmt.run(new Date().toISOString(), new Date().toISOString(), id)
    return result.changes > 0
  }

  deleteWebSocketSession(id: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM websocket_sessions WHERE id = ?')
    const result = stmt.run(id)
    return result.changes > 0
  }

  deleteWebSocketSessionsByUserId(userId: string): boolean {
    const stmt = sqliteDb.prepare('DELETE FROM websocket_sessions WHERE userId = ?')
    const result = stmt.run(userId)
    return result.changes > 0
  }

  // ===== 缓存辅助方法 =====

  /**
   * 获取缓存的企业数据
   *
   * @param id - 企业 ID
   * @returns 企业对象，如果不存在则返回 undefined
   */
  getCachedEnterpriseById(id: string): Enterprise | undefined {
    const cacheKey = `enterprise:${id}`
    let cached = this.hotColdStrategy.access(cacheKey)

    if (cached !== undefined) {
      return cached
    }

    const enterprise = this.getEnterpriseById(id)
    if (enterprise) {
      this.hotColdStrategy.access(cacheKey, enterprise)
    }
    return enterprise
  }

  /**
   * 获取缓存的用户数据
   *
   * @param id - 用户 ID
   * @returns 用户对象，如果不存在则返回 undefined
   */
  getCachedUserById(id: string): User | undefined {
    const cacheKey = `user:${id}`
    let cached = this.hotColdStrategy.access(cacheKey)

    if (cached !== undefined) {
      return cached
    }

    const user = this.getUserById(id)
    if (user) {
      this.hotColdStrategy.access(cacheKey, user)
    }
    return user
  }

  /**
   * 获取缓存的知识库文档数据
   *
   * @param id - 文档 ID
   * @returns 文档对象，如果不存在则返回 undefined
   */
  getCachedDocumentById(id: string): KnowledgeBaseDocument | undefined {
    const cacheKey = `document:${id}`
    let cached = this.hotColdStrategy.access(cacheKey)

    if (cached !== undefined) {
      return cached
    }

    const document = this.getDocumentById(id)
    if (document) {
      this.hotColdStrategy.access(cacheKey, document)
    }
    return document
  }

  /**
   * 获取缓存的代理模板数据
   *
   * @param id - 模板 ID
   * @returns 模板对象，如果不存在则返回 undefined
   */
  getCachedTemplateById(id: string): AgentTemplate | undefined {
    const cacheKey = `template:${id}`
    let cached = this.hotColdStrategy.access(cacheKey)

    if (cached !== undefined) {
      return cached
    }

    const template = this.getTemplateById(id)
    if (template) {
      this.hotColdStrategy.access(cacheKey, template)
    }
    return template
  }

  /**
   * 清除特定类型的缓存
   *
   * @param type - 缓存类型：enterprise、user、document、template、all
   */
  clearCache(type: 'enterprise' | 'user' | 'document' | 'template' | 'all' = 'all'): void {
    if (type === 'all') {
      this.cacheManager.clear()
      return
    }

    const prefix = `${type}:`
    for (const key of this.cacheManager.keys()) {
      if (key.startsWith(prefix)) {
        this.cacheManager.delete(key)
      }
    }
  }

  /**
   * 获取缓存统计信息
   */
  getCacheStats() {
    return {
      cacheStats: this.cacheManager.getStats(),
      layeringInfo: this.hotColdStrategy.getLayeringInfo(),
    }
  }

  // ===== 事件日志相关方法 =====

  /**
   * 记录事件日志
   */
  createEventLog(event: {
    id: string
    type: 'CREATE' | 'UPDATE' | 'DELETE' | 'EXECUTE'
    resourceId: string
    resourceType: string
    userId: string
    timestamp: string
    version: number
    payload: any
  }): boolean {
    const stmt = sqliteDb.prepare(`
      INSERT INTO event_logs (id, type, resourceId, resourceType, userId, timestamp, version, payload, createdAt)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    `)
    const result = stmt.run(
      event.id,
      event.type,
      event.resourceId,
      event.resourceType,
      event.userId,
      event.timestamp,
      event.version,
      JSON.stringify(event.payload),
      new Date().toISOString()
    )
    return result.changes > 0
  }

  /**
   * 获取资源的所有事件
   */
  getEventsByResourceId(resourceId: string, resourceType?: string): any[] {
    let stmt: any
    let rows: any[]

    if (resourceType) {
      stmt = sqliteDb.prepare(`
        SELECT * FROM event_logs
        WHERE resourceId = ? AND resourceType = ?
        ORDER BY version ASC
      `)
      rows = stmt.all(resourceId, resourceType) as any[]
    } else {
      stmt = sqliteDb.prepare(`
        SELECT * FROM event_logs
        WHERE resourceId = ?
        ORDER BY version ASC
      `)
      rows = stmt.all(resourceId) as any[]
    }

    return rows.map(row => ({
      ...row,
      payload: JSON.parse(row.payload),
    }))
  }

  /**
   * 获取指定时间范围内的事件
   */
  getEventsByTimestampRange(startTime: string, endTime: string): any[] {
    const stmt = sqliteDb.prepare(`
      SELECT * FROM event_logs
      WHERE timestamp >= ? AND timestamp <= ?
      ORDER BY timestamp ASC
    `)
    const rows = stmt.all(startTime, endTime) as any[]
    return rows.map(row => ({
      ...row,
      payload: JSON.parse(row.payload),
    }))
  }

  /**
   * 获取指定版本号的事件
   */
  getEventByVersion(resourceId: string, version: number): any | null {
    const stmt = sqliteDb.prepare(`
      SELECT * FROM event_logs
      WHERE resourceId = ? AND version = ?
    `)
    const row = stmt.get(resourceId, version) as any
    if (!row) return null
    return {
      ...row,
      payload: JSON.parse(row.payload),
    }
  }

  /**
   * 获取资源指定版本范围内的事件
   */
  getEventsByVersionRange(resourceId: string, startVersion: number, endVersion: number): any[] {
    const stmt = sqliteDb.prepare(`
      SELECT * FROM event_logs
      WHERE resourceId = ? AND version >= ? AND version <= ?
      ORDER BY version ASC
    `)
    const rows = stmt.all(resourceId, startVersion, endVersion) as any[]
    return rows.map(row => ({
      ...row,
      payload: JSON.parse(row.payload),
    }))
  }

  // ===== 优先级同步队列相关方法 =====

  /**
   * 将事件加入同步队列
   */
  enqueueSyncEvent(event: {
    id: string
    type: 'TOKEN_ALERT' | 'UPDATE' | 'DELETE' | 'INSERT'
    resourceId: string
    userId: string
    timestamp: string
    priority: number
    payload: any
    retries?: number
    maxRetries?: number
    status?: string
  }): boolean {
    try {
      const stmt = sqliteDb.prepare(`
        INSERT OR REPLACE INTO sync_queue
        (id, type, resourceId, userId, timestamp, priority, payload, retries, maxRetries, status, createdAt, updatedAt)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `)

      const result = stmt.run(
        event.id,
        event.type,
        event.resourceId,
        event.userId,
        event.timestamp,
        event.priority,
        JSON.stringify(event.payload),
        event.retries ?? 0,
        event.maxRetries ?? 3,
        event.status ?? 'pending',
        new Date().toISOString(),
        new Date().toISOString()
      )

      return result.changes > 0
    } catch (error) {
      console.error('加入同步队列失败:', error)
      return false
    }
  }

  /**
   * 从同步队列获取下一个事件（按优先级排序）
   */
  dequeueSyncEvent(): any | null {
    try {
      const stmt = sqliteDb.prepare(`
        SELECT * FROM sync_queue
        WHERE status = 'pending'
        ORDER BY priority ASC, timestamp ASC
        LIMIT 1
      `)

      const row = stmt.get() as any
      if (!row) return null

      return {
        ...row,
        payload: JSON.parse(row.payload),
      }
    } catch (error) {
      console.error('从同步队列获取事件失败:', error)
      return null
    }
  }

  /**
   * 更新同步事件状态
   */
  updateSyncEventStatus(id: string, status: string, error?: string): boolean {
    try {
      const stmt = sqliteDb.prepare(`
        UPDATE sync_queue
        SET status = ?, error = ?, updatedAt = ?
        WHERE id = ?
      `)

      const result = stmt.run(
        status,
        error ?? null,
        new Date().toISOString(),
        id
      )

      return result.changes > 0
    } catch (error) {
      console.error('更新同步事件状态失败:', error)
      return false
    }
  }

  /**
   * 标记同步事件为已重试
   */
  retrySyncEvent(id: string): boolean {
    try {
      const stmt = sqliteDb.prepare(`
        SELECT retries, maxRetries FROM sync_queue WHERE id = ?
      `)
      const event = stmt.get(id) as any
      if (!event) return false

      const newRetries = event.retries + 1
      const newStatus = newRetries >= event.maxRetries ? 'failed' : 'pending'

      const updateStmt = sqliteDb.prepare(`
        UPDATE sync_queue
        SET retries = ?, status = ?, updatedAt = ?
        WHERE id = ?
      `)

      const result = updateStmt.run(newRetries, newStatus, new Date().toISOString(), id)
      return result.changes > 0
    } catch (error) {
      console.error('重试同步事件失败:', error)
      return false
    }
  }

  /**
   * 获取同步队列统计信息
   */
  getSyncQueueStats(): {
    pending: number
    syncing: number
    completed: number
    failed: number
    byPriority: { [priority: number]: number }
  } {
    try {
      // 获取状态统计
      const statsStmt = sqliteDb.prepare(`
        SELECT status, COUNT(*) as count
        FROM sync_queue
        GROUP BY status
      `)
      const statRows = statsStmt.all() as any[]

      const stats = {
        pending: 0,
        syncing: 0,
        completed: 0,
        failed: 0,
        byPriority: {} as { [priority: number]: number },
      }

      for (const row of statRows) {
        stats[row.status as keyof typeof stats] = row.count
      }

      // 获取优先级统计
      const priorityStmt = sqliteDb.prepare(`
        SELECT priority, COUNT(*) as count
        FROM sync_queue
        WHERE status = 'pending'
        GROUP BY priority
      `)
      const priorityRows = priorityStmt.all() as any[]

      for (const row of priorityRows) {
        stats.byPriority[row.priority] = row.count
      }

      return stats
    } catch (error) {
      console.error('获取同步队列统计失败:', error)
      return {
        pending: 0,
        syncing: 0,
        completed: 0,
        failed: 0,
        byPriority: {},
      }
    }
  }

  /**
   * 重试失败的同步事件
   */
  retryFailedSyncEvents(maxRetries: number = 3): number {
    try {
      const stmt = sqliteDb.prepare(`
        UPDATE sync_queue
        SET status = 'pending', retries = 0, error = NULL, updatedAt = ?
        WHERE status = 'failed' AND retries < ?
      `)

      const result = stmt.run(new Date().toISOString(), maxRetries)
      return result.changes
    } catch (error) {
      console.error('重试失败的同步事件失败:', error)
      return 0
    }
  }

  /**
   * 清除已完成的同步事件
   */
  clearCompletedSyncEvents(): number {
    try {
      const stmt = sqliteDb.prepare(`
        DELETE FROM sync_queue
        WHERE status = 'completed'
      `)

      const result = stmt.run()
      return result.changes
    } catch (error) {
      console.error('清除已完成的同步事件失败:', error)
      return 0
    }
  }

  /**
   * 获取同步队列中的所有事件
   */
  getAllSyncEvents(): any[] {
    try {
      const stmt = sqliteDb.prepare(`
        SELECT * FROM sync_queue
        ORDER BY priority ASC, timestamp ASC
      `)

      const rows = stmt.all() as any[]
      return rows.map(row => ({
        ...row,
        payload: JSON.parse(row.payload),
      }))
    } catch (error) {
      console.error('获取所有同步事件失败:', error)
      return []
    }
  }

  /**
   * 按状态获取同步事件
   */
  getSyncEventsByStatus(status: string): any[] {
    try {
      const stmt = sqliteDb.prepare(`
        SELECT * FROM sync_queue
        WHERE status = ?
        ORDER BY priority ASC, timestamp ASC
      `)

      const rows = stmt.all(status) as any[]
      return rows.map(row => ({
        ...row,
        payload: JSON.parse(row.payload),
      }))
    } catch (error) {
      console.error('按状态获取同步事件失败:', error)
      return []
    }
  }
}

export const db = new Database()
