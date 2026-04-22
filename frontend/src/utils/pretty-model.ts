/**
 * 将限定模型名 `provider_id/model_id` 美化成 `provider_name / model_id`。
 * 防止把自动生成的 ID（如 `custom-1774861052113`）暴露给用户。
 *
 * @param raw 原始模型字符串
 * @param providerMap provider_id → display name 映射
 */
export function prettyModel(raw: string | undefined | null, providerMap: Record<string, string>): string {
  if (!raw) return ''
  const slash = raw.indexOf('/')
  if (slash < 0) return raw
  const pid = raw.slice(0, slash)
  const rest = raw.slice(slash + 1)
  const name = providerMap[pid]
  return name && name !== pid ? `${name} / ${rest}` : raw
}

/** React hook-friendly 工厂：从 providers 数组生成 id → name map */
export function buildProviderNameMap(providers: Array<{ id?: string; name?: string }>): Record<string, string> {
  const map: Record<string, string> = {}
  for (const p of providers || []) {
    if (p.id && p.name) map[p.id] = p.name
  }
  return map
}
