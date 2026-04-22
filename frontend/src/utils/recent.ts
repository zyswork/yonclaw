/**
 * Recent items tracking via localStorage
 *
 * 记录最近访问的 Agent / 工具 / 文件，提供快速回访。
 */

const LS_KEY = 'xianzhu.recent.v1'
const MAX_PER_KIND = 10

type Kind = 'agent' | 'tool' | 'file' | 'skill'

export interface RecentItem {
  kind: Kind
  id: string
  name: string
  accessedAt: number
  meta?: Record<string, any>
}

function readAll(): Record<Kind, RecentItem[]> {
  try {
    const raw = localStorage.getItem(LS_KEY)
    if (!raw) return { agent: [], tool: [], file: [], skill: [] }
    const parsed = JSON.parse(raw)
    return {
      agent: Array.isArray(parsed.agent) ? parsed.agent : [],
      tool: Array.isArray(parsed.tool) ? parsed.tool : [],
      file: Array.isArray(parsed.file) ? parsed.file : [],
      skill: Array.isArray(parsed.skill) ? parsed.skill : [],
    }
  } catch {
    return { agent: [], tool: [], file: [], skill: [] }
  }
}

function writeAll(store: Record<Kind, RecentItem[]>) {
  try { localStorage.setItem(LS_KEY, JSON.stringify(store)) } catch {}
}

/** 记录一次访问；相同 id 会去重并更新时间戳 */
export function trackRecent(kind: Kind, id: string, name: string, meta?: Record<string, any>) {
  if (!id) return
  const store = readAll()
  const list = store[kind].filter(x => x.id !== id)
  list.unshift({ kind, id, name, accessedAt: Date.now(), meta })
  store[kind] = list.slice(0, MAX_PER_KIND)
  writeAll(store)
}

export function getRecent(kind: Kind, limit = MAX_PER_KIND): RecentItem[] {
  return readAll()[kind].slice(0, limit)
}

export function clearRecent(kind?: Kind) {
  if (!kind) {
    writeAll({ agent: [], tool: [], file: [], skill: [] })
    return
  }
  const store = readAll()
  store[kind] = []
  writeAll(store)
}
