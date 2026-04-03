// 认证路由

import { Router, Request, Response } from 'express'
import { v4 as uuidv4 } from 'uuid'
import bcrypt from 'bcryptjs'
import { db } from '../db/index.js'
import { generateToken } from '../middleware/auth.js'
import { validateRequest } from '../middleware/validator.js'
import { authSchemas } from '../middleware/validation.js'
import { User } from '../models/user.js'
import { generateCode, sendVerificationCode, verifyCode } from '../services/mail.js'

const router = Router()

// 默认企业 ID（验证码注册的用户归入此企业）
const DEFAULT_ENTERPRISE_ID = '001'

// ========== 验证码流程 ==========

// 发送验证码
router.post('/send-code', async (req: Request, res: Response) => {
  try {
    const { email } = req.body

    if (!email || typeof email !== 'string') {
      res.status(400).json({ error: '请提供有效的邮箱地址' })
      return
    }

    // 简单邮箱格式验证
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
    if (!emailRegex.test(email)) {
      res.status(400).json({ error: '邮箱格式不正确' })
      return
    }

    const code = generateCode()
    const directCode = await sendVerificationCode(email, code)

    if (directCode) {
      // SMTP 未配置，验证码直接返回（用户无需查邮件）
      console.log(`[认证] 验证码直接返回: ${email}`)
      res.json({ message: '验证码已生成', expiresIn: 300, code: directCode })
    } else {
      console.log(`[认证] 验证码已发送至 ${email}`)
      res.json({ message: '验证码已发送', expiresIn: 300 })
    }
  } catch (error) {
    console.error('发送验证码失败:', error)
    res.status(500).json({ error: '发送验证码失败，请稍后重试' })
  }
})

// 验证码验证（统一注册/登录入口）
router.post('/verify-code', (req: Request, res: Response) => {
  try {
    const { email, code } = req.body

    if (!email || !code) {
      res.status(400).json({ error: '请提供邮箱和验证码' })
      return
    }

    // 校验验证码
    if (!verifyCode(email, code)) {
      res.status(401).json({ error: '验证码错误或已过期' })
      return
    }

    // 查找用户
    let user = db.getUserByEmail(email)

    if (!user) {
      // 用户不存在 → 自动注册
      // 确保默认企业存在
      let enterprise = db.getEnterpriseById(DEFAULT_ENTERPRISE_ID)
      if (!enterprise) {
        // 自动创建默认企业
        db.createEnterprise({
          id: DEFAULT_ENTERPRISE_ID,
          name: '衔烛默认组织',
          description: '默认企业',
          status: 'active',
          createdAt: new Date(),
          updatedAt: new Date(),
        })
      }

      const newUser: User = {
        id: `user_${uuidv4()}`,
        enterpriseId: DEFAULT_ENTERPRISE_ID,
        email,
        name: email.split('@')[0], // 用邮箱前缀作为默认昵称
        role: 'user',
        permissions: [],
        status: 'active',
        createdAt: new Date(),
        updatedAt: new Date(),
      }

      user = db.createUser(newUser)
      console.log(`[认证] 新用户注册: ${email}`)
    } else {
      console.log(`[认证] 用户登录: ${email}`)
    }

    // 生成 token
    const token = generateToken({
      id: user.id,
      email: user.email,
      enterpriseId: user.enterpriseId,
      role: user.role,
    })

    res.json({
      token,
      user: {
        id: user.id,
        email: user.email,
        name: user.name,
        role: user.role,
        enterpriseId: user.enterpriseId,
      },
      isNewUser: !user.lastLogin,
    })
  } catch (error) {
    console.error('验证码验证失败:', error)
    res.status(500).json({ error: '验证失败' })
  }
})

// 设置密码（新用户注册后调用）
router.post('/set-password', (req: Request, res: Response) => {
  try {
    const { email, password, token: authToken } = req.body

    if (!email || !password) {
      res.status(400).json({ error: '请提供邮箱和密码' })
      return
    }
    if (password.length < 6) {
      res.status(400).json({ error: '密码至少 6 个字符' })
      return
    }

    const user = db.getUserByEmail(email)
    if (!user) {
      res.status(404).json({ error: '用户不存在' })
      return
    }

    // 哈希密码并更新
    const passwordHash = bcrypt.hashSync(password, 10)
    db.updateUser(user.id, { passwordHash, name: req.body.name || user.name })

    console.log(`[认证] 用户设置密码: ${email}`)
    res.json({ message: '密码设置成功' })
  } catch (error) {
    console.error('设置密码失败:', error)
    res.status(500).json({ error: '设置密码失败' })
  }
})

// ========== 原有密码登录 ==========

// 登录
router.post('/login', validateRequest(authSchemas.login), (req: Request, res: Response) => {
  try {
    const { email, password, enterpriseId } = req.body

    // 验证企业是否存在
    const enterprise = db.getEnterpriseById(enterpriseId)
    if (!enterprise) {
      res.status(404).json({ error: '企业不存在' })
      return
    }

    // 根据 email 和 enterpriseId 查询用户
    const user = db.getUserByEmailAndEnterprise(email, enterpriseId)

    if (!user) {
      res.status(401).json({ error: '用户名或密码错误' })
      return
    }

    // 验证密码
    if (!user.passwordHash || !bcrypt.compareSync(password, user.passwordHash)) {
      res.status(401).json({ error: '用户名或密码错误' })
      return
    }

    // 生成 token
    const token = generateToken({
      id: user.id,
      email: user.email,
      enterpriseId: user.enterpriseId,
      role: user.role,
    })

    res.json({
      token,
      user: {
        id: user.id,
        email: user.email,
        name: user.name,
        role: user.role,
        enterpriseId: user.enterpriseId,
      },
    })
  } catch (error) {
    console.error('登录失败:', error)
    res.status(500).json({ error: '登录失败' })
  }
})

// 注册
router.post('/register', validateRequest(authSchemas.register), (req: Request, res: Response) => {
  try {
    const { email, name, password, enterpriseId } = req.body

    // 验证企业是否存在
    const enterprise = db.getEnterpriseById(enterpriseId)
    if (!enterprise) {
      res.status(404).json({ error: '企业不存在' })
      return
    }

    // 检查用户是否已存在
    const existingUser = db.getUserByEmailAndEnterprise(email, enterpriseId)
    if (existingUser) {
      res.status(400).json({ error: '用户已存在' })
      return
    }

    // 哈希密码
    const passwordHash = bcrypt.hashSync(password, 10)

    // 创建新用户
    const newUser: User = {
      id: `user_${uuidv4()}`,
      enterpriseId,
      email,
      name,
      passwordHash,
      role: 'user',
      permissions: [],
      status: 'active',
      createdAt: new Date(),
      updatedAt: new Date(),
    }

    const created = db.createUser(newUser)

    // 生成 token
    const token = generateToken({
      id: created.id,
      email: created.email,
      enterpriseId: created.enterpriseId,
      role: created.role,
    })

    res.status(201).json({
      token,
      user: {
        id: created.id,
        email: created.email,
        name: created.name,
        role: created.role,
        enterpriseId: created.enterpriseId,
      },
    })
  } catch (error) {
    console.error('注册失败:', error)
    res.status(500).json({ error: '注册失败' })
  }
})

export default router
