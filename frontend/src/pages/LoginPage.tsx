/**
 * 登录/注册页
 *
 * 流程：邮箱 → 验证码 → 登录 → 设置密码（可选）
 * 也支持密码直接登录
 */

import { useState, useEffect } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'
import { useAuthStore } from '../store/authStore'
import { useI18n } from '../i18n'
import { authAPI } from '../api/auth'

type Step = 'email' | 'code' | 'password-login' | 'set-password'

export default function LoginPage() {
  const { t } = useI18n()
  const navigate = useNavigate()
  const { login, isLoggedIn } = useAuthStore()
  const [step, setStep] = useState<Step>('email')
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [name, setName] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [countdown, setCountdown] = useState(0)
  // 验证码登录成功后暂存 auth 数据，不立即调 login()（避免 isLoggedIn 提前变 true 触发跳转）
  const [pendingAuth, setPendingAuth] = useState<{ token: string; user: any } | null>(null)

  // 验证码倒计时
  useEffect(() => {
    if (countdown <= 0) return
    const timer = setTimeout(() => setCountdown(c => c - 1), 1000)
    return () => clearTimeout(timer)
  }, [countdown])

  // ★ 关键：渲染时同步判断，不用 useEffect（避免 Zustand/React 时序问题）
  // 已登录 + 不在设密码步骤 → 直接重定向（<Navigate> 是同步渲染）
  if (isLoggedIn && step !== 'set-password') {
    return <Navigate to="/agents" replace />
  }

  // 发送验证码
  const handleSendCode = async () => {
    if (!email.trim() || !email.includes('@')) {
      setError(t('login.emailInvalid')); return
    }
    setLoading(true); setError('')
    try {
      const res = await authAPI.sendCode(email.trim())
      setCountdown(res.data.expiresIn || 60)
      setStep('code')
      // SMTP 未配置时，后端直接返回验证码，自动填入
      if (res.data.code) {
        setCode(res.data.code)
      }
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || t('login.sendCodeFailed'))
    } finally { setLoading(false) }
  }

  // 验证码登录
  const handleVerifyCode = async () => {
    if (!code.trim()) { setError(t('login.codeRequired')); return }
    setLoading(true); setError('')
    try {
      const res = await authAPI.verifyCode(email.trim(), code.trim())
      if (res.data.isNewUser) {
        // 新用户：暂存 auth，先设密码
        setPendingAuth({ token: res.data.token, user: res.data.user })
        setStep('set-password')
      } else {
        // 老用户：直接登录
        login(res.data.token, res.data.user)
      }
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || t('login.verifyFailed'))
    } finally { setLoading(false) }
  }

  // 密码登录
  const handlePasswordLogin = async () => {
    if (!email.trim()) { setError(t('login.emailInvalid')); return }
    if (!password.trim()) { setError(t('login.passwordRequired')); return }
    setLoading(true); setError('')
    try {
      const res = await authAPI.login('001', email.trim(), password.trim())
      login(res.data.token, res.data.user)
      // 密码登录不需要设密码，直接进首页（下次渲染 Navigate 会处理）
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || t('login.loginFailed'))
    } finally { setLoading(false) }
  }

  // 设置密码
  const handleSetPassword = async () => {
    if (!password.trim() || password.length < 6) {
      setError(t('login.passwordTooShort')); return
    }
    setLoading(true); setError('')
    try {
      await authAPI.setPassword(email.trim(), password.trim(), name.trim() || undefined)
      // 设完密码，正式登录并跳首页
      if (pendingAuth) login(pendingAuth.token, pendingAuth.user)
      navigate('/agents', { replace: true })
    } catch (e: any) {
      setError(e.response?.data?.error || e.message)
    } finally { setLoading(false) }
  }

  const inputStyle = {
    width: '100%', padding: '10px 12px', fontSize: 14,
    border: '1px solid var(--border-subtle, #2a2a3e)',
    borderRadius: 8, backgroundColor: 'var(--bg-primary, #0a0a14)',
    color: 'var(--text-primary, #fff)', outline: 'none',
    boxSizing: 'border-box' as const,
  }
  const btnStyle = {
    width: '100%', padding: 10, fontSize: 14, fontWeight: 600,
    border: 'none', borderRadius: 8, cursor: loading ? 'not-allowed' : 'pointer',
    background: 'var(--accent-gradient, linear-gradient(135deg, #10b981, #06b6d4))',
    color: '#fff', opacity: loading ? 0.6 : 1,
  }
  const linkStyle = {
    width: '100%', padding: 8, fontSize: 12, border: 'none', borderRadius: 8,
    cursor: 'pointer', background: 'transparent', color: 'var(--text-muted, #888)',
  }

  return (
    <div style={{
      minHeight: '100vh', display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'var(--bg-primary, #0a0a14)',
    }}>
      <div style={{
        width: 380, padding: 32, borderRadius: 16,
        background: 'var(--bg-elevated, #1a1a2e)',
        border: '1px solid var(--border-subtle, #2a2a3e)',
        boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
      }}>
        <div style={{ textAlign: 'center', marginBottom: 24 }}>
          <h2 style={{ margin: '0 0 8px', fontSize: 22, fontWeight: 700, color: 'var(--text-primary, #fff)' }}>
            {t('login.title')}
          </h2>
          <p style={{ margin: 0, fontSize: 13, color: 'var(--text-muted, #888)' }}>
            {step === 'email' && t('login.subtitleEmail')}
            {step === 'code' && t('login.subtitleCode')}
            {step === 'password-login' && t('login.subtitlePassword')}
            {step === 'set-password' && t('login.subtitleSetPassword')}
          </p>
        </div>

        {error && <div style={{ fontSize: 12, color: 'var(--error, #ef4444)', marginBottom: 12 }}>{error}</div>}

        {/* 邮箱输入 */}
        {step === 'email' && (<>
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 12, color: 'var(--text-secondary, #aaa)', marginBottom: 6 }}>{t('login.emailLabel')}</label>
            <input type="email" value={email} onChange={e => setEmail(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) handleSendCode() }}
              placeholder={t('login.emailPlaceholder')} autoFocus style={inputStyle} />
          </div>
          <button onClick={handleSendCode} disabled={loading} style={{ ...btnStyle, marginBottom: 8 }}>
            {loading ? t('common.loading') : t('login.sendCodeBtn')}
          </button>
          <button onClick={() => { setStep('password-login'); setError('') }} style={linkStyle}>
            {t('login.usePasswordLogin')}
          </button>
        </>)}

        {/* 验证码 */}
        {step === 'code' && (<>
          <div style={{ marginBottom: 8, fontSize: 13, color: 'var(--text-secondary, #aaa)' }}>
            {t('login.codeSentTo')} <strong style={{ color: 'var(--text-primary, #fff)' }}>{email}</strong>
          </div>
          <div style={{ marginBottom: 16 }}>
            <input type="text" value={code} onChange={e => setCode(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) handleVerifyCode() }}
              placeholder={t('login.codePlaceholder')} autoFocus maxLength={6}
              style={{ ...inputStyle, letterSpacing: 8, textAlign: 'center', fontSize: 20 }} />
          </div>
          <button onClick={handleVerifyCode} disabled={loading} style={{ ...btnStyle, marginBottom: 8 }}>
            {loading ? t('common.loading') : t('login.verifyBtn')}
          </button>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12 }}>
            <button onClick={() => setStep('email')} style={{ background: 'none', border: 'none', color: 'var(--text-muted)', cursor: 'pointer' }}>
              {t('login.changeEmail')}
            </button>
            <button onClick={handleSendCode} disabled={countdown > 0 || loading}
              style={{ background: 'none', border: 'none', color: countdown > 0 ? 'var(--text-muted)' : 'var(--accent)', cursor: countdown > 0 ? 'default' : 'pointer' }}>
              {countdown > 0 ? `${t('login.resend')} (${countdown}s)` : t('login.resend')}
            </button>
          </div>
        </>)}

        {/* 密码登录 */}
        {step === 'password-login' && (<>
          <div style={{ marginBottom: 12 }}>
            <label style={{ display: 'block', fontSize: 12, color: 'var(--text-secondary, #aaa)', marginBottom: 6 }}>{t('login.emailLabel')}</label>
            <input type="email" value={email} onChange={e => setEmail(e.target.value)} placeholder={t('login.emailPlaceholder')} autoFocus style={inputStyle} />
          </div>
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 12, color: 'var(--text-secondary, #aaa)', marginBottom: 6 }}>{t('login.passwordLabel')}</label>
            <input type="password" value={password} onChange={e => setPassword(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) handlePasswordLogin() }}
              placeholder={t('login.passwordPlaceholder')} style={inputStyle} />
          </div>
          <button onClick={handlePasswordLogin} disabled={loading} style={{ ...btnStyle, marginBottom: 8 }}>
            {loading ? t('common.loading') : t('login.loginBtn')}
          </button>
          <button onClick={() => { setStep('email'); setError('') }} style={linkStyle}>
            {t('login.useCodeLogin')}
          </button>
        </>)}

        {/* 设置密码 */}
        {step === 'set-password' && (<>
          <div style={{ padding: '8px 12px', marginBottom: 16, borderRadius: 8, background: 'rgba(16,185,129,0.1)', border: '1px solid rgba(16,185,129,0.2)', fontSize: 13, color: 'var(--accent, #10b981)' }}>
            {t('login.loginSuccess') || '登录成功！建议设置密码方便下次登录。'}
          </div>
          <div style={{ marginBottom: 12 }}>
            <label style={{ display: 'block', fontSize: 12, color: 'var(--text-secondary, #aaa)', marginBottom: 6 }}>{t('login.nameLabel')}</label>
            <input type="text" value={name} onChange={e => setName(e.target.value)} placeholder={t('login.namePlaceholder')} autoFocus style={inputStyle} />
          </div>
          <div style={{ marginBottom: 16 }}>
            <label style={{ display: 'block', fontSize: 12, color: 'var(--text-secondary, #aaa)', marginBottom: 6 }}>{t('login.passwordLabel')}</label>
            <input type="password" value={password} onChange={e => setPassword(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) handleSetPassword() }}
              placeholder={t('login.newPasswordPlaceholder')} style={inputStyle} />
          </div>
          <button onClick={handleSetPassword} disabled={loading} style={{ ...btnStyle, marginBottom: 8 }}>
            {loading ? t('common.loading') : t('login.setPasswordBtn')}
          </button>
          <button onClick={() => { if (pendingAuth) login(pendingAuth.token, pendingAuth.user); navigate('/agents', { replace: true }) }} style={linkStyle}>
            {t('login.skipSetPassword')}
          </button>
        </>)}
      </div>
    </div>
  )
}
