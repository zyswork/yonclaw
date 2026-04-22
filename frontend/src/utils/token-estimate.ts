/**
 * 粗略估算文本的 token 数（前端版）
 *
 * 规则：
 * - 中文 1 字 ≈ 1.5 token
 * - 英文 1 词 ≈ 1.3 token
 * - 数字/符号约 0.5 token
 *
 * 精确值应由后端的 tiktoken / cl100k 计算，这里是输入时的快速反馈。
 */
export function estimateTokens(text: string): number {
  if (!text) return 0
  let cjk = 0
  let ascii = 0
  for (const ch of text) {
    const code = ch.codePointAt(0) || 0
    // CJK 统一汉字 + 扩展 A/B
    if (
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0x3400 && code <= 0x4dbf) ||
      (code >= 0x20000 && code <= 0x2a6df)
    ) {
      cjk++
    } else {
      ascii++
    }
  }
  const asciiWords = (text.match(/[a-zA-Z]+/g) || []).length
  const numbers = (text.match(/\d+/g) || []).length
  return Math.ceil(cjk * 1.5 + asciiWords * 1.3 + numbers * 0.5 + ascii * 0.05)
}

/**
 * 估算成本（USD）
 *
 * 输入 token 单价表 —— 只列常用模型，未命中按 0 返回（不显示价格）。
 */
const PRICE_PER_1K: Record<string, number> = {
  'gpt-4o': 0.005, 'gpt-4o-mini': 0.00015,
  'gpt-5': 0.005, 'gpt-5-turbo': 0.002,
  'claude-opus-4': 0.015, 'claude-sonnet-4': 0.003, 'claude-haiku-4': 0.00025,
  'claude-3-5-sonnet': 0.003, 'claude-3-haiku': 0.00025,
  'deepseek-chat': 0.00014, 'deepseek-reasoner': 0.00055,
  'qwen-turbo': 0.00015, 'qwen-max': 0.0012,
  'gemini-2.5-pro': 0.00125, 'gemini-2.5-flash': 0.000075,
  'glm-4.7': 0.0005, 'glm-4.7-flash': 0.0001,
}

export function estimateCostUsd(text: string, model: string): number | null {
  const tokens = estimateTokens(text)
  const m = (model || '').toLowerCase()
  for (const [prefix, price] of Object.entries(PRICE_PER_1K)) {
    if (m.includes(prefix)) {
      return (tokens / 1000) * price
    }
  }
  return null
}

export function formatCost(usd: number | null): string {
  if (usd === null) return ''
  if (usd < 0.0001) return '<$0.0001'
  return `$${usd.toFixed(4)}`
}
