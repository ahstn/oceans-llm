#!/usr/bin/env node
// Maps a git diff to the verification proofs in features/routing.json.
// Usage: plan-verification.mjs [base-ref] [--json]
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const skillDir = join(dirname(fileURLToPath(import.meta.url)), '..')
const control = '.agents/skills/verify-oceans-admin/scripts/control-oceans-admin'

export function globToRegExp(glob) {
  let source = ''
  for (let i = 0; i < glob.length; i += 1) {
    const char = glob[i]
    if (char === '*' && glob[i + 1] === '*') {
      const slash = glob[i + 2] === '/'
      source += slash ? '(?:.*/)?' : '.*'
      i += slash ? 2 : 1
    } else if (char === '*') {
      source += '[^/]*'
    } else if (char === '?') {
      source += '[^/]'
    } else if (char === '{') {
      const end = glob.indexOf('}', i)
      source += `(?:${glob.slice(i + 1, end).split(',').map(escapeRegExp).join('|')})`
      i = end
    } else {
      source += escapeRegExp(char)
    }
  }
  return new RegExp(`^${source}$`)
}

function escapeRegExp(text) {
  return text.replace(/[.+^${}()|[\]\\]/g, '\\$&')
}

function matcher(globs) {
  const patterns = globs.map(globToRegExp)
  return (path) => patterns.some((pattern) => pattern.test(path))
}

export function planFor(paths, routing) {
  const skips = routing.skip.map((rule) => ({ ...rule, test: matcher(rule.paths) }))
  const features = routing.features.map((feature) => ({ ...feature, test: matcher(feature.paths) }))
  const shared = routing.shared.map((rule) => ({ ...rule, test: matcher(rule.paths) }))
  const selected = new Map()
  const skipped = []
  const unmapped = []

  const select = (id, path) => {
    if (!selected.has(id)) selected.set(id, new Set())
    selected.get(id).add(path)
  }

  for (const path of [...new Set(paths)].sort()) {
    const skip = skips.find((rule) => rule.test(path))
    if (skip) {
      skipped.push({ path, reason: skip.reason })
      continue
    }
    let matched = false
    for (const feature of features) {
      if (feature.test(path)) {
        select(feature.id, path)
        matched = true
      }
    }
    for (const rule of shared) {
      if (rule.test(path)) {
        rule.features.forEach((id) => select(id, path))
        matched = true
      }
    }
    if (!matched) unmapped.push(path)
  }

  const proofs = routing.features
    .filter((feature) => selected.has(feature.id))
    .map((feature) => ({
      id: feature.id,
      file: feature.file,
      proof: feature.proof,
      paid: Boolean(feature.paid),
      because: [...selected.get(feature.id)],
    }))
  return { proofs, skipped, unmapped, noRuntimeSurface: proofs.length === 0 && unmapped.length === 0 }
}

function git(args) {
  return execFileSync('git', args, { encoding: 'utf8' }).trim()
}

function changedPaths(base) {
  const mergeBase = git(['merge-base', base, 'HEAD'])
  const tracked = git(['diff', '--name-only', mergeBase])
  const untracked = git(['ls-files', '--others', '--exclude-standard'])
  return { mergeBase, paths: `${tracked}\n${untracked}`.split('\n').filter(Boolean) }
}

function defaultBase(routing) {
  for (const ref of routing.baseRefs) {
    try {
      git(['rev-parse', '--verify', '--quiet', ref])
      return ref
    } catch {
      // Try the next candidate.
    }
  }
  throw new Error(`None of ${routing.baseRefs.join(', ')} exist; pass a base ref.`)
}

function proofCommand(proof) {
  if (proof.drive) return `${control} drive ${proof.drive}`
  return `manual recipe (use Playwright or a browser against the launched stack)`
}

function printPlan(base, mergeBase, plan) {
  console.log(`Verification plan for ${base} (merge-base ${mergeBase.slice(0, 12)}) plus working tree`)
  for (const proof of plan.proofs) {
    const paid = proof.paid ? ' [paid: follow the Live LLM requests policy]' : ''
    console.log(`\nRUN ${proof.id}${paid}`)
    console.log(`  recipe: .agents/skills/verify-oceans-admin/features/${proof.file}`)
    console.log(`  proof:  ${proofCommand(proof.proof)}`)
    console.log(`  because: ${proof.because.join(', ')}`)
  }
  if (plan.unmapped.length > 0) {
    console.log('\nUNMAPPED (decide the runtime surface, then add the path to features/routing.json):')
    plan.unmapped.forEach((path) => console.log(`  ${path}`))
  }
  if (plan.skipped.length > 0) {
    console.log('\nSKIPPED:')
    plan.skipped.forEach(({ path, reason }) => console.log(`  ${path} (${reason})`))
  }
  if (plan.noRuntimeSurface) {
    console.log('\nSKIP: no runtime surface. Report this line instead of a live proof.')
  }
}

function main(argv) {
  const json = argv.includes('--json')
  const routing = JSON.parse(readFileSync(join(skillDir, 'features/routing.json'), 'utf8'))
  const base = argv.find((arg) => !arg.startsWith('--')) ?? defaultBase(routing)
  const { mergeBase, paths } = changedPaths(base)
  const plan = planFor(paths, routing)
  if (json) {
    console.log(JSON.stringify({ base, mergeBase, ...plan }, null, 2))
  } else {
    printPlan(base, mergeBase, plan)
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2))
}
