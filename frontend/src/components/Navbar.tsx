/**
 * 导航栏组件
 *
 * 显示应用标题和用户操作按钮
 */

import { useNavigate } from 'react-router-dom'
import { useI18n } from '../i18n'

export default function Navbar() {
  const { t } = useI18n()
  const navigate = useNavigate()

  const handleLogout = () => {
    localStorage.removeItem('token')
    navigate('/login')
  }

  return (
    <nav style={{
      display: 'flex',
      justifyContent: 'space-between',
      alignItems: 'center',
      padding: '16px 24px',
      backgroundColor: '#f5f5f5',
      borderBottom: '1px solid #e0e0e0',
    }}>
      <h1 style={{ margin: 0, fontSize: '20px', fontWeight: 600 }}>YonClaw</h1>
      <button
        onClick={handleLogout}
        style={{
          padding: '8px 16px',
          backgroundColor: '#ff4444',
          color: 'white',
          border: 'none',
          borderRadius: '4px',
          cursor: 'pointer',
          fontSize: '14px',
        }}
      >
        {t('common.logout')}
      </button>
    </nav>
  )
}
