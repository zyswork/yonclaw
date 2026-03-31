import express, { Express, Request, Response, NextFunction, ErrorRequestHandler } from 'express'
import http from 'http'
import cors from 'cors'
import compression from 'compression'
import dotenv from 'dotenv'
import path from 'path'
import { fileURLToPath } from 'url'
import {
  enterprisesRouter,
  usersRouter,
  knowledgeBaseRouter,
  agentTemplatesRouter,
  tokenMonitoringRouter,
  searchRouter,
} from './routes/mod.js'
import authRouter from './routes/auth.js'
import telemetryRouter from './routes/telemetry.js'
import updateRouter from './routes/update.js'
import { setupBridgeWebSocket } from './routes/bridge.js'
import { authMiddleware } from './middleware/auth.js'
import { isAppError } from './utils/errors.js'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

dotenv.config()

// 环境变量验证
function validateEnvironment() {
  const requiredEnvVars = ['JWT_SECRET']
  const missingEnvVars = requiredEnvVars.filter(
    (envVar) => !process.env[envVar]
  )

  if (missingEnvVars.length > 0) {
    console.error(
      `❌ 缺少必需的环���变量: ${missingEnvVars.join(', ')}`
    )
    console.error('请在 .env 文件中设置这些变量')
    process.exit(1)
  }

  console.log('✓ 环境变量验证通过')
}

const app: Express = express()
const port = process.env.PORT || 3000

// 内存监控 - 记录内存使用情况
function logMemoryUsage(label: string) {
  if (typeof process !== 'undefined' && process.memoryUsage) {
    const memUsage = process.memoryUsage()
    const heapUsedMB = (memUsage.heapUsed / 1024 / 1024).toFixed(2)
    const heapTotalMB = (memUsage.heapTotal / 1024 / 1024).toFixed(2)
    console.log(`[内存] ${label}: ${heapUsedMB}MB / ${heapTotalMB}MB`)
  }
}

// 中间件
app.use(cors({
  origin: process.env.CORS_ORIGIN || 'http://localhost:5173',
}))

// 启用响应压缩 - 减少网络传输时间
app.use(compression({
  level: 6, // 压缩级别 (0-9)，6 是平衡速度和压缩率
  threshold: 1024, // 仅压缩大于 1KB 的响应
}))

app.use(express.json({ limit: '10mb' })) // 限制请求体大小以减少内存占用

// 性能监控中间件 - 记录响应时间超过 100ms 的请求
app.use((req: Request, res: Response, next: NextFunction) => {
  const startTime = Date.now()
  res.on('finish', () => {
    const duration = Date.now() - startTime
    if (duration > 100) {
      console.log(`⚠️  [性能] ${req.method} ${req.path} - ${duration}ms`)
    }
  })
  next()
})

// 请求日志中间件
app.use((req: Request, res: Response, next: NextFunction) => {
  console.log(`[${new Date().toISOString()}] ${req.method} ${req.path}`)
  next()
})

// 健康检查
app.get('/health', (req: Request, res: Response) => {
  res.json({ status: 'ok', timestamp: new Date().toISOString() })
})

// API 路由
app.get('/api/v1/info', (req: Request, res: Response) => {
  res.json({
    name: 'XianZhu Admin Backend',
    version: '0.2.0',
    description: '企业级开源协作平台后台 API',
  })
})

// 认证路由（不需要认证）
app.use('/api/v1/auth', authRouter)

// 以下路由需要认证
app.use('/api/v1/enterprises', authMiddleware, enterprisesRouter)

// 用户管理 API
app.use('/api/v1/users', authMiddleware, usersRouter)

// 知识库管理 API
app.use('/api/v1/knowledge-base', authMiddleware, knowledgeBaseRouter)

// Agent 模板管理 API
app.use('/api/v1/agent-templates', authMiddleware, agentTemplatesRouter)

// Token 监控 API
app.use('/api/v1/token-monitoring', authMiddleware, tokenMonitoringRouter)

// 搜索 API（向量搜索）
app.use('/api/v1/search', authMiddleware, searchRouter)

// 遥测 API（部分端点无需认证，认证在路由内部处理）
app.use('/api/v1/telemetry', telemetryRouter)
app.use('/api/v1/update', updateRouter)

// 管理后台静态页面（HTML 文件在 src/ 目录下，不被 tsc 编译，需要从 src 目录引用）
app.use('/admin', express.static(path.join(__dirname, '..', 'src', 'admin-ui')))

// 404 处理
app.use((req: Request, res: Response) => {
  res.status(404).json({
    error: '未找到请求的资源',
    code: 'NOT_FOUND',
    path: req.path,
    method: req.method,
  })
})

// 错误处理中间件（必须是最后一个，且必须有 4 个参数）
const errorHandler: ErrorRequestHandler = (err: Error, req: Request, res: Response, next: NextFunction) => {
  console.error('[ERROR]', err.message, err.stack)

  if (isAppError(err)) {
    res.status(err.statusCode).json({
      error: err.message,
      code: err.code,
    })
    return
  }

  // 处理其他错误
  res.status(500).json({
    error: '内部服务器错误',
    code: 'INTERNAL_SERVER_ERROR',
  })
}
app.use(errorHandler)

// 应用启动
async function main() {
  const startTime = Date.now()
  console.log('⏱️  后端启动开始')
  logMemoryUsage('启动前')

  // 验证环境变量
  validateEnvironment()

  // 使用 http.createServer 以支持 WebSocket upgrade
  const server = http.createServer(app)

  // Bridge WebSocket — 桌面端通过 /ws/bridge 连接
  setupBridgeWebSocket(server)

  server.listen(port, () => {
    const elapsed = Date.now() - startTime
    logMemoryUsage('启动后')
    console.log(`✓ 后端启动完成，耗时: ${elapsed}ms`)
    console.log(`✓ 后端服务启动成功: http://localhost:${port}`)
  })
}

main().catch((error) => {
  console.error('❌ 应用启动失败:', error)
  process.exit(1)
})
