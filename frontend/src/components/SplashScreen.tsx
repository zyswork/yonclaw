import { useI18n } from '../i18n'
import styles from './SplashScreen.module.css'

interface SplashScreenProps {
  message?: string
  progress?: number
}

export default function SplashScreen({
  message,
  progress = 0
}: SplashScreenProps) {
  const { t } = useI18n()
  const displayMessage = message ?? t('common.loading')
  return (
    <div className={styles.container} data-testid="splash-screen">
      <div className={styles.content}>
        {/* 加载动画 */}
        <div className={styles.spinner} data-testid="splash-spinner" />

        {/* 应用名称 */}
        <h1 className={styles.title}>衔烛</h1>

        {/* 状态消息 */}
        <p className={styles.message}>{displayMessage}</p>

        {/* 进度条 */}
        {progress > 0 && (
          <div className={styles.progressContainer}>
            <progress
              className={styles.progressBar}
              value={progress}
              max={100}
              data-testid="splash-progress"
              aria-valuenow={progress}
            />
            <span className={styles.progressText}>{progress}%</span>
          </div>
        )}
      </div>
    </div>
  )
}
