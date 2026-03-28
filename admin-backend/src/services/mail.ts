// 邮件服务 — 验证码发送与校验

import nodemailer from 'nodemailer'

const transporter = nodemailer.createTransport({
  host: process.env.SMTP_HOST || 'smtp-relay.brevo.com',
  port: parseInt(process.env.SMTP_PORT || '587'),
  secure: false,
  auth: {
    user: process.env.SMTP_USER,
    pass: process.env.SMTP_PASS,
  },
})

// 验证码内存缓存（email -> { code, expires }）
const codeCache = new Map<string, { code: string; expires: number }>()

/** 生成 6 位数字验证码 */
export function generateCode(): string {
  return Math.floor(100000 + Math.random() * 900000).toString()
}

/** 发送验证码邮件 */
export async function sendVerificationCode(email: string, code: string): Promise<void> {
  // 存缓存，5 分钟过期
  codeCache.set(email, { code, expires: Date.now() + 5 * 60 * 1000 })

  await transporter.sendMail({
    from: `"衔烛 XianZhu" <${process.env.SMTP_FROM || process.env.SMTP_USER}>`,
    to: email,
    subject: '衔烛 - 邮箱验证码',
    html: `
      <div style="max-width:400px;margin:0 auto;padding:30px;font-family:sans-serif;background:#0a0a0f;color:#f0f0f5;border-radius:12px;">
        <h2 style="color:#34d399;margin:0 0 20px;">衔烛 XianZhu</h2>
        <p>您的验证码是：</p>
        <div style="font-size:32px;font-weight:700;letter-spacing:8px;color:#34d399;background:rgba(52,211,153,0.1);padding:16px 24px;border-radius:8px;text-align:center;margin:16px 0;">
          ${code}
        </div>
        <p style="color:#a0a0b0;font-size:13px;">验证码 5 分钟内有效，请勿泄露给他人。</p>
        <hr style="border:none;border-top:1px solid rgba(255,255,255,0.06);margin:20px 0;" />
        <p style="color:#606070;font-size:11px;">此邮件由衔烛 AI 助手自动发送，请勿回复。</p>
      </div>
    `,
  })
}

/** 校验验证码（一次性，验证后即删除） */
export function verifyCode(email: string, code: string): boolean {
  const cached = codeCache.get(email)
  if (!cached) return false
  if (Date.now() > cached.expires) {
    codeCache.delete(email)
    return false
  }
  if (cached.code !== code) return false
  codeCache.delete(email) // 一次性使用
  return true
}
