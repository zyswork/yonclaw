import { describe, it, expect } from 'vitest'
import { prettyModel, buildProviderNameMap } from '../utils/pretty-model'

describe('prettyModel', () => {
  it('returns raw when no slash', () => {
    expect(prettyModel('gpt-4o', {})).toBe('gpt-4o')
  })

  it('replaces provider_id with name when mapped', () => {
    const map = { 'custom-1774861052113': 'mimo' }
    expect(prettyModel('custom-1774861052113/mimo-v2-pro', map)).toBe('mimo / mimo-v2-pro')
  })

  it('keeps raw when provider id not in map', () => {
    expect(prettyModel('openai/gpt-4o', {})).toBe('openai/gpt-4o')
  })

  it('does not replace when name equals id', () => {
    const map = { 'openai': 'openai' }
    expect(prettyModel('openai/gpt-4o', map)).toBe('openai/gpt-4o')
  })

  it('handles empty/null input', () => {
    expect(prettyModel('', {})).toBe('')
    expect(prettyModel(undefined, {})).toBe('')
    expect(prettyModel(null, {})).toBe('')
  })

  it('handles slash as first char', () => {
    expect(prettyModel('/foo', {})).toBe('/foo')
  })

  it('only replaces first slash segment', () => {
    const map = { 'a': 'Alpha' }
    expect(prettyModel('a/b/c', map)).toBe('Alpha / b/c')
  })
})

describe('buildProviderNameMap', () => {
  it('builds id → name map', () => {
    const map = buildProviderNameMap([
      { id: 'openai', name: 'OpenAI' },
      { id: 'anthropic', name: 'Anthropic' },
    ])
    expect(map).toEqual({ openai: 'OpenAI', anthropic: 'Anthropic' })
  })

  it('skips entries missing id or name', () => {
    const map = buildProviderNameMap([
      { id: 'x' },
      { name: 'Y' },
      { id: 'z', name: 'Z' },
    ] as any)
    expect(map).toEqual({ z: 'Z' })
  })

  it('handles empty input', () => {
    expect(buildProviderNameMap([])).toEqual({})
    expect(buildProviderNameMap(null as any)).toEqual({})
  })
})
