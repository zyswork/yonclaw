import { useState, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/tauri'

/**
 * 语音输入 Hook
 *
 * 通过 Tauri 后端 Swift 录音，再调用 Whisper API 转文字。
 */
export function useVoiceInput() {
  const [isRecording, setIsRecording] = useState(false)
  const [isTranscribing, setIsTranscribing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const startRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke<string>('start_voice_recording')
      setIsRecording(true)
    } catch (e) {
      setError(String(e))
    }
  }, [])

  const stopRecording = useCallback(async (): Promise<string> => {
    try {
      // 停止录音，获取文件路径
      const filePath = await invoke<string>('stop_voice_recording')
      setIsRecording(false)
      setIsTranscribing(true)

      // 后端直接用文件路径转录（不需要前端读文件）
      const text = await invoke<string>('transcribe_audio_file', {
        filePath, language: null,
      })
      setIsTranscribing(false)
      return text
    } catch (e) {
      setIsRecording(false)
      setIsTranscribing(false)
      setError(String(e))
      return ''
    }
  }, [])

  const cancelRecording = useCallback(async () => {
    try {
      await invoke<string>('stop_voice_recording')
    } catch {}
    setIsRecording(false)
    setIsTranscribing(false)
  }, [])

  return { isRecording, isTranscribing, startRecording, stopRecording, cancelRecording, error }
}
