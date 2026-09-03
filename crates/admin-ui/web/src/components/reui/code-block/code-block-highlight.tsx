import type { CSSProperties } from 'react'

export type CodeBlockToken = {
  content: string
  color?: string
  colorDark?: string
  fontStyle?: 'italic' | 'bold' | 'underline'
}

export type CodeBlockLine = {
  number: number
  tokens: CodeBlockToken[]
}

type Highlighter = {
  codeToHast: (code: string, options: Record<string, unknown>) => unknown
  loadLanguage: (language: unknown) => Promise<void>
  loadTheme: (theme: unknown) => Promise<void>
}

type HastNode = {
  type: string
  tagName?: string
  value?: string
  properties?: Record<string, unknown>
  children?: HastNode[]
}

const LANGUAGE_LOADERS: Record<string, () => Promise<unknown>> = {
  json: () => import('shiki/langs/json.mjs'),
  toml: () => import('shiki/langs/toml.mjs'),
}

const THEME_LOADERS: Record<string, () => Promise<unknown>> = {
  'github-light': () => import('shiki/themes/github-light.mjs'),
  'github-dark': () => import('shiki/themes/github-dark.mjs'),
}

const loadedLanguages = new Set<string>()
const loadedThemes = new Set<string>()
let highlighterPromise: Promise<Highlighter> | null = null

async function loadHighlighter() {
  if (!highlighterPromise) {
    highlighterPromise = Promise.all([
      import('shiki/core'),
      import('shiki/engine/javascript'),
    ]).then(async ([{ createHighlighterCore }, { createJavaScriptRegexEngine }]) =>
      createHighlighterCore({
        themes: [],
        langs: [],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      }),
    ) as Promise<Highlighter>
  }
  return highlighterPromise
}

export function resolveCodeBlockLanguage(language?: string) {
  if (!language) return undefined
  const normalized = language.trim().toLowerCase()
  return normalized in LANGUAGE_LOADERS ? normalized : undefined
}

export function toPlainLines(code: string): CodeBlockLine[] {
  return code
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((content, index) => ({
      number: index + 1,
      tokens: content ? [{ content }] : [],
    }))
}

async function ensureLanguage(highlighter: Highlighter, language: string) {
  if (loadedLanguages.has(language)) return
  await highlighter.loadLanguage(await LANGUAGE_LOADERS[language]())
  loadedLanguages.add(language)
}

async function ensureTheme(highlighter: Highlighter, theme: string) {
  if (loadedThemes.has(theme)) return
  await highlighter.loadTheme(await THEME_LOADERS[theme]())
  loadedThemes.add(theme)
}

function findCodeElement(node: HastNode): HastNode | undefined {
  if (node.type === 'element' && node.tagName === 'code') return node
  for (const child of node.children ?? []) {
    const code = findCodeElement(child)
    if (code) return code
  }
  return undefined
}

function tokenStyle(style: unknown): Omit<CodeBlockToken, 'content'> {
  if (typeof style !== 'string') return {}

  const token: Omit<CodeBlockToken, 'content'> = {}
  for (const declaration of style.split(';')) {
    const [rawProperty, ...rawValue] = declaration.split(':')
    const property = rawProperty?.trim()
    const value = rawValue.join(':').trim()
    if (!property || !value) continue

    if (property === 'color') token.color = value
    else if (property === '--shiki-dark') token.colorDark = value
    else if (property === 'font-style' && value === 'italic') token.fontStyle = 'italic'
    else if (property === 'font-weight' && value === 'bold') token.fontStyle = 'bold'
    else if (property === 'text-decoration' && value === 'underline') {
      token.fontStyle = 'underline'
    }
  }
  return token
}

function collectTokens(node: HastNode, tokens: CodeBlockToken[]): void {
  for (const child of node.children ?? []) {
    if (child.type === 'text') {
      if (child.value) tokens.push({ content: child.value })
      continue
    }
    if (child.type !== 'element') continue

    const textOnly = (child.children ?? []).every((grandchild) => grandchild.type === 'text')
    const style = tokenStyle(child.properties?.style)
    if (textOnly && Object.keys(style).length > 0) {
      const content = (child.children ?? []).map((grandchild) => grandchild.value ?? '').join('')
      if (content) tokens.push({ content, ...style })
      continue
    }
    collectTokens(child, tokens)
  }
}

export async function highlightCode(code: string, language?: string): Promise<CodeBlockLine[]> {
  const resolvedLanguage = resolveCodeBlockLanguage(language)
  if (!resolvedLanguage || !code) return toPlainLines(code)

  try {
    const highlighter = await loadHighlighter()
    await Promise.all([
      ensureLanguage(highlighter, resolvedLanguage),
      ensureTheme(highlighter, 'github-light'),
      ensureTheme(highlighter, 'github-dark'),
    ])
    const root = highlighter.codeToHast(code, {
      lang: resolvedLanguage,
      themes: { light: 'github-light', dark: 'github-dark' },
      defaultColor: 'light',
    }) as HastNode
    const codeElement = findCodeElement(root)
    if (!codeElement) return toPlainLines(code)

    const lines: CodeBlockLine[] = []
    for (const child of codeElement.children ?? []) {
      if (child.type !== 'element') continue
      const tokens: CodeBlockToken[] = []
      collectTokens(child, tokens)
      lines.push({ number: lines.length + 1, tokens })
    }
    return lines.length > 0 ? lines : toPlainLines(code)
  } catch {
    return toPlainLines(code)
  }
}

export function codeBlockTokenStyle(token: CodeBlockToken): CSSProperties {
  return {
    '--code-token-light': token.color ?? 'currentColor',
    '--code-token-dark': token.colorDark ?? token.color ?? 'currentColor',
    fontStyle: token.fontStyle === 'italic' ? 'italic' : undefined,
    fontWeight: token.fontStyle === 'bold' ? 700 : undefined,
    textDecoration: token.fontStyle === 'underline' ? 'underline' : undefined,
  } as CSSProperties
}
