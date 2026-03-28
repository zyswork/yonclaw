import { useState, useCallback, useRef } from 'react'

/**
 * 去除 Markdown 标记，返回纯文本用于语音朗读
 */
function stripMarkdown(text: string): string {
  return text
    .replace(/```[\s\S]*?```/g, '')          // 代码块
    .replace(/`[^`]*`/g, '')                 // 内联代码
    .replace(/#+\s/g, '')                    // 标题
    .replace(/\*\*([^*]+)\*\*/g, '$1')       // 粗体
    .replace(/\*([^*]+)\*/g, '$1')           // 斜体
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1') // 链接
    .replace(/!\[.*?\]\(.*?\)/g, '')         // 图片
    .replace(/[-*_]{3,}/g, '')               // 分割线
    .replace(/^\s*[-*+]\s/gm, '')            // 无序列表
    .replace(/^\s*\d+\.\s/gm, '')            // 有序列表
    .replace(/>\s?/g, '')                    // 引用
    .trim()
}

/**
 * 语音输出 Hook
 *
 * 使用浏览器 SpeechSynthesis API 朗读文本，自动检测中英文。
 */
export function useVoiceOutput() {
  const [isSpeaking, setIsSpeaking] = useState(false)
  const [voiceEnabled, setVoiceEnabledState] = useState(
    () => localStorage.getItem('xianzhu.voiceEnabled') === 'true',
  )
  const utterRef = useRef<SpeechSynthesisUtterance | null>(null)

  const speak = useCallback((text: string) => {
    if (!text || !window.speechSynthesis) return
    window.speechSynthesis.cancel()

    const cleaned = stripMarkdown(text)
    if (!cleaned) return

    const utter = new SpeechSynthesisUtterance(cleaned)
    // 检测语言
    const hasChinese = /[\u4e00-\u9fff]/.test(cleaned)
    utter.lang = hasChinese ? 'zh-CN' : 'en-US'
    utter.rate = 1.0
    utter.onstart = () => setIsSpeaking(true)
    utter.onend = () => setIsSpeaking(false)
    utter.onerror = () => setIsSpeaking(false)
    utterRef.current = utter
    window.speechSynthesis.speak(utter)
  }, [])

  const stop = useCallback(() => {
    window.speechSynthesis.cancel()
    setIsSpeaking(false)
  }, [])

  const toggleVoice = useCallback((v: boolean) => {
    setVoiceEnabledState(v)
    localStorage.setItem('xianzhu.voiceEnabled', v ? 'true' : 'false')
    if (!v) window.speechSynthesis.cancel()
  }, [])

  return { isSpeaking, speak, stop, voiceEnabled, setVoiceEnabled: toggleVoice }
}
