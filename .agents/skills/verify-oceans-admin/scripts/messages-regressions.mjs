import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { existsSync, lstatSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..')

export const requiredChecks = [
  ['native-display-and-efforts', 'native_opus_5_5_preserves_adaptive_display_and_all_efforts'],
  ['omitted-controls', 'native_opus_5_5_leaves_omitted_thinking_and_effort_to_upstream_defaults'],
  ['field-specific-errors', 'native_opus_5_5_reports_invalid_display_separately_from_manual_budgets'],
  ['native-metadata', 'native_response_round_trip_preserves_opaque_safeguard_results_and_tool_ids'],
  ['per-message-effort', 'native_effort_messages_and_safeguards_survive_provider_mapping'],
  ['bedrock-messages', 'native_messages_preserves_thinking_controls_across_bedrock_transports'],
  ['vertex-http', 'vertex_native_messages_preserves_opus_5_5_thinking_controls_over_http'],
  ['gateway-accounting', 'inference_endpoints_normalize_models_and_account_for_success'],
  ['gateway-budget-rejection', 'inference_endpoints_reject_exhausted_budget_before_dispatch'],
  ['native-stream-metadata', 'native_anthropic_stream_round_trip_preserves_safeguard_results'],
  ['stream-metadata-without-stop', 'messages_stream_preserves_pending_metadata_without_finish_reason'],
  ['caller-beta-ingress', 'request_headers_preserve_repeated_anthropic_beta_opt_ins'],
  ['caller-beta-wire', 'native_messages_beta_controls_reach_each_provider_over_http'],
  ['count-tokens-beta-wire', 'native_count_tokens_forwards_caller_beta_headers_over_http'],
  ['beta-resource-limits', 'beta_merge_rejects_resource_limit_overflows'],
  ['beta-rejection-before-dispatch', 'beta_resource_limits_prevent_provider_http_dispatch'],
  ['separate-api-thinking', 'chat_completions_and_messages_use_separate_thinking_contracts'],
  ['bedrock-api-thinking', 'bedrock_chat_completions_and_messages_use_separate_thinking_contracts'],
  ['vertex-chat-default', 'vertex_chat_completions_defaults_adaptive_only_models_to_high'],
  ['trusted-api-ingress', 'chat_and_messages_handlers_set_trusted_protocol'],
  ['trusted-api-context', 'request_json_cannot_override_trusted_chat_protocol'],
]

export function assessResults(output, exitCode) {
  const passed = new Set([...output.matchAll(/^test ([\w:]+) \.\.\. ok$/gm)].map((match) => match[1]))
  const checks = requiredChecks.map(([id, name]) => ({
    id,
    status: [...passed].some((test) => test.endsWith(`::${name}`)) ? 'pass' : 'fail',
  }))
  checks.push({ id: 'rust-suites-completed', status: exitCode === 0 && passed.size > 0 ? 'pass' : 'fail' })
  return { verdict: checks.every((check) => check.status === 'pass') ? 'pass' : 'fail', passedTests: passed.size, checks }
}

// Includes tests and Rust suite dependencies, unlike runtime-only planner fingerprints.
export function regressionFingerprint(root = repoRoot) {
  const git = (args, input) => execFileSync('git', args, { cwd: root, input, encoding: 'utf8' }).trim()
  const files = [...new Set(git(['ls-files', '--cached', '--others', '--exclude-standard', '--',
    'Cargo.toml', 'Cargo.lock', 'crates', 'gateway.yaml',
    '.agents/skills/verify-oceans-admin/scripts',
    '.agents/skills/verify-oceans-admin/features/routing.json',
  ]).split('\n').filter((file) => file && existsSync(path.join(root, file))))].sort()
  const blobs = files.length ? git(['hash-object', '--stdin-paths'], files.join('\n')).split('\n') : []
  const hash = createHash('sha256')
  files.forEach((file, index) => {
    const stat = lstatSync(path.join(root, file))
    const mode = stat.isSymbolicLink() ? '120000' : stat.mode & 0o111 ? '100755' : '100644'
    hash.update(`${file}\0${mode} ${blobs[index]}\n`)
  })
  return { sourceHash: hash.digest('hex'), sourceFiles: files.length }
}

export function assessMessagesRegressions(proof, evidenceDir, requireCurrent) {
  const file = 'messages-regressions.json'
  if (!evidenceDir || !proof.artifacts?.includes(file)) return ['Messages proof requires messages-regressions.json']
  let artifact
  try { artifact = JSON.parse(readFileSync(path.join(evidenceDir, file), 'utf8')) }
  catch { return ['Messages regression artifact is missing or unreadable'] }
  const required = [...requiredChecks.map(([id]) => id), 'rust-suites-completed']
  if (artifact?.schemaVersion !== 1 || artifact.verdict !== 'pass' || !(artifact.passedTests > 0)
    || !Array.isArray(artifact.checks)
    || !required.every((id) => artifact.checks.some((check) => check?.id === id && check.status === 'pass'))
    || artifact.checks.some((check) => check?.status !== 'pass')) {
    return ['Messages regression artifact did not pass all required checks']
  }
  if (requireCurrent && artifact.sourceHash !== regressionFingerprint().sourceHash) {
    return ['stale: Messages regression source or tests changed; rerun check-messages.mjs']
  }
  return []
}
