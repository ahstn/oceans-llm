import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { existsSync, lstatSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..')

export const requiredChecks = [
  ['gemini-terminal-observation', 'google_stream_call_then_eof_is_not_success_and_preserves_usage_and_replay_parts'],
  ['gemini-terminal-precedence', 'google_stream_terminal_truncation_or_filter_wins_over_calls_in_either_order'],
  ['gemini-generation-failure-usage', 'google_stream_failed_generation_consumes_late_usage_without_reopening_content'],
  ['gemini-malformed-json-usage', 'vertex_provider_google_rejected_generation_preserves_billable_usage'],
  ['gemini-parallel-results', 'idless_parallel_results_follow_call_ids_even_when_function_names_match'],
  ['gemini-strict-controls', 'strict_function_control_is_rejected_before_tool_choice_can_discard_it'],
  ['gemini-strict-response-schema', 'strict_response_schema_is_rejected_even_with_native_response_controls'],
  ['gemini-logprobs', 'returned_logprobs_keep_chosen_tokens_separate_from_top_candidates'],
  ['gemini-ordered-replay', 'google_stream_extension_aware_replay_restores_order_and_rejects_edits'],
  ['gemini-foreign-replay', 'google_replay_ignores_foreign_vertex_anthropic_native_content'],
  ['anthropic-ordered-replay', 'json_replay_preserves_ordered_signed_tools_server_results_citations_and_pause'],
  ['anthropic-indexed-stream-replay', 'stream_replay_has_indexed_boundaries_complete_ordered_blocks_and_incremental_chat'],
  ['anthropic-context-limit', 'context_window_exceeded_stream_is_length_with_native_stop_state'],
  ['anthropic-incomplete-replay-usage', 'incomplete_tool_replay_drains_final_usage_and_preserves_truthful_terminal_outcome'],
  ['anthropic-replay-control-validation', 'replay_rejects_explicit_public_text_block_fields_that_would_be_discarded'],
  ['bedrock-malformed-generation-usage', 'malformed_generation_emits_later_usage_then_error_without_success'],
  ['bedrock-truncated-tool-usage', 'converse_partial_tool_json_preserves_truncation_filter_or_deferred_error_and_usage'],
  ['bedrock-anthropic-replay-usage', 'independent_anthropic_replay_errors_drain_same_event_and_later_usage'],
  ['bedrock-ordered-replay', 'converse_chat_roundtrip_preserves_signed_redacted_and_future_block_order'],
  ['bedrock-parallel-results', 'converse_groups_parallel_results_without_merging_unrelated_user_turns'],
  ['bedrock-response-fields', 'converse_requested_additional_response_fields_remain_available'],
  ['bedrock-crc-validation', 'validates_independent_ieee_checksums_and_rejects_each_corruption'],
  ['bedrock-header-types', 'accepts_all_legal_typed_headers_and_rejects_truncated_values'],
  ['bedrock-crc-prefix-usage', 'eventstream_valid_usage_prefix_survives_later_crc_failure_independent_of_chunking'],
  ['bedrock-dense-tool-indices', 'sparse_interleaved_tool_blocks_keep_dense_stable_chat_ordinals'],
  ['chat-cache-aliases', 'root_chat_cache_aliases_bill_reads_and_writes_without_billing_misses_as_writes'],
  ['chat-cache-canonical-precedence', 'canonical_cache_fields_win_per_field_including_zero_and_partial_details'],
  ['chat-cache-cost-precedence', 'provider_reported_cost_wins_over_valid_or_invalid_root_cache_aliases'],
  ['replay-response-guardrail-invalidation', 'response_transforms_invalidate_ordered_replay_metadata'],
  ['replay-stream-guardrail-invalidation', 'stream_transforms_invalidate_later_replay_snapshots_without_losing_usage'],
  ['native-transport-fidelity', 'native_messages_preserve_bodies_headers_and_sse_across_transports'],
  ['native-json-utf8', 'native_json_rejects_invalid_utf8_instead_of_replacing_it'],
  ['native-json-body-integrity', 'native_json_rejects_truncated_http_body_even_after_complete_json'],
  ['native-json-body-deadline', 'native_json_body_deadline_remains_a_timeout_after_complete_json'],
  ['rolling-alias-parity', 'rolling_native_controls_match_explicit_models_and_unknown_fields_reach_upstream'],
  ['native-gateway-accounting', 'native_messages_handlers_preserve_protocol_and_account_for_cached_usage'],
  ['native-gateway-guardrails', 'native_messages_generated_tools_still_pass_through_guardrails'],
  ['native-stream-completion', 'native_messages_truncated_stream_fails_and_accounts_for_observed_usage'],
  ['native-chat-cache-totals', 'openrouter_stream_totals_include_cached_tokens_across_usage_events'],
  ['openrouter-remote-images', 'openrouter_chat_maps_remote_images_to_native_url_sources'],
  ['native-route-token-caps', 'route_token_aliases_override_caller_limits_without_unsupported_fields'],
  ['unknown-model-manual-thinking', 'unknown_native_models_preserve_manual_thinking_with_reasoning_aliases'],
  ['native-system-prompt-guardrails', 'denies_plain_system_prompt_without_other_text_fields'],
  ['native-sse-tool-guardrails', 'denies_generated_tools_for_every_sse_line_ending'],
  ['native-start-tool-denial', 'native_boundary_denies_complete_tool_start_input_without_json_deltas'],
  ['native-start-tool-transform', 'native_boundary_transforms_complete_tool_start_input_as_object_preserving_terminal_frames'],
  ['native-start-delta-denial', 'native_boundary_denies_conflicting_initial_or_delta_tool_input_independently'],
  ['native-start-delta-transform', 'native_boundary_transforms_conflicting_initial_and_delta_tool_inputs_independently'],
  ['native-remote-image-cache', 'native_boundary_remote_image_outer_cache_policy_survives_openrouter_mapping'],
  ['native-data-image-cache', 'native_boundary_data_image_outer_cache_policy_survives_mapping'],
  ['native-base64-image-cache', 'native_boundary_base64_image_outer_cache_policy_survives_mapping'],
  ['native-sse-text-guardrails', 'denies_response_text_for_every_sse_line_ending'],
  ['native-multiline-start', 'native_model_rewrite_preserves_framing_and_all_other_json_bytes'],
  ['openrouter-chat-json-schema', 'openrouter_chat_json_schema_uses_native_output_format'],
  ['openrouter-chat-format-conflicts', 'openrouter_chat_json_schema_rejects_conflicting_native_format'],
  ['native-output-config-fidelity', 'openrouter_native_messages_preserve_output_config'],
  ['mantle-authoritative-version', 'builds_mantle_anthropic_messages_request_with_authoritative_envelope'],
  ['mixed-ttl-usage-counts', 'mixed_ttl_cache_usage_keeps_valid_counts_when_pricing_is_unsupported'],
  ['mixed-ttl-cost-precedence', 'mixed_ttl_usage_retains_ledger_counts_and_provider_reported_cost_precedence'],
  ['mixed-ttl-rate-limits', 'native_mixed_ttl_tokens_exhaust_limits_for_json_and_streaming'],
  ['native-model-discovery', 'openrouter_claude_routes_advertise_native_messages'],
  ['rolling-client-config', 'rolling_claude_aliases_generate_adaptive_effort_clients'],
  ['native-chat-cost', 'native_openrouter_costs_survive_chat_completions_translation'],
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
