// SQLite 数据库实现

import Database from 'better-sqlite3'
import bcrypt from 'bcryptjs'
import path from 'path'
import { fileURLToPath } from 'url'
import fs from 'fs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const dbPath = path.join(__dirname, '../../data/openclaw.db')

// 确保数据目录存在
const dataDir = path.dirname(dbPath)
if (!fs.existsSync(dataDir)) {
  fs.mkdirSync(dataDir, { recursive: true })
}

const sqliteDb: any = new Database(dbPath)

// 启用外键约束
sqliteDb.pragma('foreign_keys = ON')

// 初始化数据库 schema
export function initializeDatabase() {
  // 企业表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS enterprises (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      description TEXT,
      logo TEXT,
      website TEXT,
      industry TEXT,
      size TEXT,
      status TEXT DEFAULT 'active',
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL
    )
  `)

  // 用户表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS users (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL,
      email TEXT NOT NULL UNIQUE,
      name TEXT NOT NULL,
      passwordHash TEXT,
      role TEXT DEFAULT 'member',
      status TEXT DEFAULT 'active',
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id)
    )
  `)

  // 迁移：为已有数据库添加 passwordHash 字段
  const userColumns = sqliteDb.pragma('table_info(users)') as any[]
  const hasPasswordHash = userColumns.some((col: any) => col.name === 'passwordHash')
  if (!hasPasswordHash) {
    sqliteDb.exec('ALTER TABLE users ADD COLUMN passwordHash TEXT')
    console.log('已迁移: 为 users 表添加 passwordHash 字段')
  }

  // 知识库文档表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS knowledge_base_documents (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL,
      title TEXT NOT NULL,
      content TEXT NOT NULL,
      contentType TEXT DEFAULT 'text',
      tags TEXT,
      permissions TEXT,
      version INTEGER DEFAULT 1,
      status TEXT DEFAULT 'draft',
      createdBy TEXT NOT NULL,
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL,
      vectorized INTEGER DEFAULT 0,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id)
    )
  `)

  // Agent 模板表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS agent_templates (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL,
      name TEXT NOT NULL,
      description TEXT,
      category TEXT,
      config TEXT,
      version TEXT,
      status TEXT DEFAULT 'draft',
      permissions TEXT,
      createdBy TEXT NOT NULL,
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL,
      publishedAt TEXT,
      tags TEXT,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id)
    )
  `)

  // Token 使用记录表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS token_usage (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL,
      userId TEXT NOT NULL,
      model TEXT NOT NULL,
      inputTokens INTEGER,
      outputTokens INTEGER,
      totalTokens INTEGER,
      cost REAL,
      timestamp TEXT NOT NULL,
      requestId TEXT,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id),
      FOREIGN KEY (userId) REFERENCES users(id)
    )
  `)

  // Token 配额表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS token_quotas (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL UNIQUE,
      monthlyLimit INTEGER,
      dailyLimit INTEGER,
      currentMonthUsage INTEGER DEFAULT 0,
      currentDayUsage INTEGER DEFAULT 0,
      resetDate TEXT,
      status TEXT DEFAULT 'active',
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id)
    )
  `)

  // Token 告警表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS token_alerts (
      id TEXT PRIMARY KEY,
      enterpriseId TEXT NOT NULL,
      type TEXT NOT NULL,
      threshold INTEGER,
      enabled INTEGER DEFAULT 1,
      notificationChannels TEXT,
      createdAt TEXT NOT NULL,
      updatedAt TEXT NOT NULL,
      FOREIGN KEY (enterpriseId) REFERENCES enterprises(id)
    )
  `)

  // 用户状态历史表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS user_status_history (
      id TEXT PRIMARY KEY,
      userId TEXT NOT NULL,
      oldStatus TEXT,
      newStatus TEXT NOT NULL,
      reason TEXT,
      changedBy TEXT NOT NULL,
      createdAt TEXT NOT NULL,
      FOREIGN KEY (userId) REFERENCES users(id)
    )
  `)

  // WebSocket 会话表
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS websocket_sessions (
      id TEXT PRIMARY KEY,
      userId TEXT NOT NULL,
      connectedAt TEXT NOT NULL,
      lastHeartbeat TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'connected',
      createdAt TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updatedAt TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY (userId) REFERENCES users(id)
    )
  `)

  // 事件日志表 - 事件溯源存储
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS event_logs (
      id TEXT PRIMARY KEY,
      type TEXT NOT NULL CHECK (type IN ('CREATE', 'UPDATE', 'DELETE', 'EXECUTE')),
      resourceId TEXT NOT NULL,
      resourceType TEXT NOT NULL,
      userId TEXT NOT NULL,
      timestamp TEXT NOT NULL,
      version INTEGER NOT NULL,
      payload TEXT NOT NULL,
      createdAt TEXT NOT NULL,
      FOREIGN KEY (userId) REFERENCES users(id)
    )
  `)

  // 同步队列表 - 优先级同步队列存储
  sqliteDb.exec(`
    CREATE TABLE IF NOT EXISTS sync_queue (
      id TEXT PRIMARY KEY,
      type TEXT NOT NULL CHECK (type IN ('TOKEN_ALERT', 'UPDATE', 'DELETE', 'INSERT')),
      resourceId TEXT NOT NULL,
      userId TEXT NOT NULL,
      timestamp TEXT NOT NULL,
      priority INTEGER NOT NULL DEFAULT 2,
      payload TEXT NOT NULL,
      retries INTEGER NOT NULL DEFAULT 0,
      maxRetries INTEGER NOT NULL DEFAULT 3,
      status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'syncing', 'completed', 'failed')),
      error TEXT,
      createdAt TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updatedAt TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

      CONSTRAINT unique_sync UNIQUE(resourceId, userId, timestamp)
    )
  `)

  // 创建索引
  sqliteDb.exec(`
    CREATE INDEX IF NOT EXISTS idx_users_enterprise ON users(enterpriseId);
    CREATE INDEX IF NOT EXISTS idx_documents_enterprise ON knowledge_base_documents(enterpriseId);
    CREATE INDEX IF NOT EXISTS idx_templates_enterprise ON agent_templates(enterpriseId);
    CREATE INDEX IF NOT EXISTS idx_token_usage_enterprise ON token_usage(enterpriseId);
    CREATE INDEX IF NOT EXISTS idx_token_usage_timestamp ON token_usage(timestamp);
    CREATE INDEX IF NOT EXISTS idx_user_status_history_user ON user_status_history(userId);
    CREATE INDEX IF NOT EXISTS idx_user_status_history_created ON user_status_history(createdAt);
    CREATE INDEX IF NOT EXISTS idx_websocket_sessions_user ON websocket_sessions(userId);
    CREATE INDEX IF NOT EXISTS idx_websocket_sessions_status ON websocket_sessions(status);
    CREATE INDEX IF NOT EXISTS idx_websocket_sessions_heartbeat ON websocket_sessions(lastHeartbeat);
    CREATE INDEX IF NOT EXISTS idx_event_logs_resource ON event_logs(resourceId, resourceType);
    CREATE INDEX IF NOT EXISTS idx_event_logs_timestamp ON event_logs(timestamp);
    CREATE INDEX IF NOT EXISTS idx_event_logs_version ON event_logs(version);
    CREATE INDEX IF NOT EXISTS idx_sync_queue_priority_status ON sync_queue(priority ASC, status, timestamp ASC);
    CREATE INDEX IF NOT EXISTS idx_sync_queue_status ON sync_queue(status);
    CREATE INDEX IF NOT EXISTS idx_sync_queue_timestamp ON sync_queue(timestamp);
  `)

  // 种子数据：默认企业和管理员（仅在企业表为空时插入）
  const count = sqliteDb.prepare('SELECT COUNT(*) as cnt FROM enterprises').get()
  if (count.cnt === 0) {
    const now = new Date().toISOString()
    const defaultPasswordHash = bcrypt.hashSync('admin123', 10)
    sqliteDb.prepare(`
      INSERT INTO enterprises (id, name, description, industry, size, status, createdAt, updatedAt)
      VALUES ('001', 'XianZhu', 'XianZhu AI Platform', 'Technology', 'small', 'active', ?, ?)
    `).run(now, now)
    sqliteDb.prepare(`
      INSERT INTO users (id, enterpriseId, email, name, passwordHash, role, status, createdAt, updatedAt)
      VALUES ('user_admin', '001', 'admin@xianzhu.com', 'Admin', ?, 'admin', 'active', ?, ?)
    `).run(defaultPasswordHash, now, now)
    console.log('已创建默认企业(001)和管理员(admin@xianzhu.com, 默认密码: admin123)')
  }

  // 迁移：为已有 admin 用户设置默认密码（如果没有密码）
  const adminUser = sqliteDb.prepare('SELECT id, passwordHash FROM users WHERE id = ?').get('user_admin') as any
  if (adminUser && !adminUser.passwordHash) {
    const defaultPasswordHash = bcrypt.hashSync('admin123', 10)
    sqliteDb.prepare('UPDATE users SET passwordHash = ? WHERE id = ?').run(defaultPasswordHash, 'user_admin')
    console.log('已为 admin 用户设置默认密码: admin123')
  }
}

export { sqliteDb as default }
export const db = sqliteDb as any
