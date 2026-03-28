import { create } from 'zustand'
import type { Locale, TranslationMap } from './types'
import { DEFAULT_LOCALE, SUPPORTED_LOCALES } from './types'
import { zhCN } from './locales/zh-CN'
import { en } from './locales/en'

const translations: Record<Locale, TranslationMap> = { 'zh-CN': zhCN, 'en': en }

function resolveInitialLocale(): Locale {
  const saved = localStorage.getItem('xianzhu.locale')
  if (saved && SUPPORTED_LOCALES.includes(saved as Locale)) return saved as Locale
  const nav = navigator.language
  if (nav.startsWith('zh')) return 'zh-CN'
  return 'en'
}

function resolve(map: TranslationMap, key: string): string | undefined {
  const parts = key.split('.')
  let v: unknown = map
  for (const k of parts) {
    if (v && typeof v === 'object') v = (v as Record<string, unknown>)[k]
    else return undefined
  }
  return typeof v === 'string' ? v : undefined
}

interface I18nState {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: (key: string, params?: Record<string, string | number>) => string
}

export const useI18n = create<I18nState>((set, get) => ({
  locale: resolveInitialLocale(),
  setLocale: (locale: Locale) => {
    localStorage.setItem('xianzhu.locale', locale)
    set({ locale })
  },
  t: (key: string, params?: Record<string, string | number>) => {
    const { locale } = get()
    let value = resolve(translations[locale], key)
    if (value === undefined && locale !== DEFAULT_LOCALE) {
      value = resolve(translations[DEFAULT_LOCALE], key)
    }
    if (value === undefined) return key
    if (params) {
      return value.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? `{${k}}`))
    }
    return value
  },
}))
