/**
 * Prompt 库（localStorage 持久化）
 *
 * 用户可以收藏常用 prompt，在输入框快速插入。
 */

const LS_KEY = 'xianzhu.promptLibrary.v1'
const MAX_SLOTS = 50

export interface Prompt {
  id: string
  title: string
  content: string
  createdAt: number
  usedCount: number
}

const DEFAULT_PROMPTS: Omit<Prompt, 'id' | 'createdAt' | 'usedCount'>[] = [
  { title: '解释这段代码', content: '请解释以下代码的工作原理，包括关键算法、边界条件和性能特征。\n\n```\n\n```' },
  { title: '代码审查', content: '审查以下代码，关注：\n1. 正确性\n2. 性能\n3. 可维护性\n4. 潜在 bug\n\n```\n\n```' },
  { title: '翻译成英文', content: '请把以下内容翻译成地道的英文，保留技术术语：\n\n' },
  { title: '写测试', content: '为以下代码写完整的单元测试，覆盖正常路径、边界和异常：\n\n```\n\n```' },
  { title: '优化性能', content: '找出以下代码的性能瓶颈并给出优化方案：\n\n```\n\n```' },
]

export function loadPrompts(): Prompt[] {
  try {
    const raw = localStorage.getItem(LS_KEY)
    if (raw) {
      const parsed = JSON.parse(raw)
      if (Array.isArray(parsed)) return parsed
    }
  } catch {}
  // 首次：注入默认模板
  const now = Date.now()
  const seeded: Prompt[] = DEFAULT_PROMPTS.map((p, i) => ({
    ...p,
    id: `default-${i}`,
    createdAt: now,
    usedCount: 0,
  }))
  savePrompts(seeded)
  return seeded
}

export function savePrompts(list: Prompt[]) {
  try { localStorage.setItem(LS_KEY, JSON.stringify(list.slice(0, MAX_SLOTS))) } catch {}
}

export function addPrompt(title: string, content: string): Prompt {
  const p: Prompt = { id: `p-${Date.now()}`, title, content, createdAt: Date.now(), usedCount: 0 }
  const list = loadPrompts()
  list.unshift(p)
  savePrompts(list)
  return p
}

export function deletePrompt(id: string) {
  savePrompts(loadPrompts().filter(p => p.id !== id))
}

export function incrementUsed(id: string) {
  const list = loadPrompts().map(p => p.id === id ? { ...p, usedCount: p.usedCount + 1 } : p)
  savePrompts(list)
}
