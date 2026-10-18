// Candidate file contract for `drive mcp`. Validation runs before the browser starts.

// Each candidate registers a server and makes bounded upstream calls, so the cap limits
// quota and run time. Six fits the example: one public server, GitHub, and Exa and ReUI each
// paired with a negative control.
export const MAX_CANDIDATES = 6;
const MAX_RESULT_MARKERS = 4;

export function validateCandidates(value) {
  if (!Array.isArray(value?.candidates) || value.candidates.length < 1 || value.candidates.length > MAX_CANDIDATES) throw new Error(`Supply one to ${MAX_CANDIDATES} MCP candidates, including any negative-auth control`);
  if (!value.candidates.some((candidate) => candidate.required !== false)) throw new Error("At least one MCP candidate must be required");
  const seen = new Set();
  for (const candidate of value.candidates) {
    if (!/^[a-z0-9][a-z0-9-]{0,30}$/.test(candidate.key) || seen.has(candidate.key)) throw new Error("Candidate key must be unique and URL-safe");
    seen.add(candidate.key);
    if (typeof candidate.label !== "string" || !candidate.label.trim()) throw new Error("Candidate label is required");
    const url = new URL(candidate.server_url);
    if (url.protocol !== "https:" || url.username || url.password || url.hash) throw new Error("Candidate endpoint must be HTTPS without URL credentials");
    if ([...url.searchParams.keys()].some((key) => /key|token|secret|auth/i.test(key))) throw new Error("Do not place credentials in the MCP URL");
    if (!["none", "gateway_static_header", "gateway_bearer_token"].includes(candidate.auth_mode)) throw new Error("This proof supports public or gateway-authenticated discovery only");
    const auth = candidate.auth_config ?? {};
    if (Object.keys(auth).some((key) => !["secret_ref", "header_name", "value_template", "query_param"].includes(key))) throw new Error("Auth config must contain only an environment reference and an optional header name, value template, or query parameter name");
    if (candidate.auth_mode !== "none" && !/^env\/OCEANS_MCP_DISCOVERY_[A-Z0-9_]+$/.test(auth.secret_ref ?? "")) throw new Error("Auth secret_ref must use env/OCEANS_MCP_DISCOVERY_*");
    const control = candidate.expect_tool_error || candidate.expect_discovery_error;
    if (control && candidate.auth_mode === "none") throw new Error("Negative-auth control requires an authenticated server mode");
    if (candidate.expect_tool_error && candidate.expect_discovery_error) throw new Error("A control expects either a tool error or a discovery error, not both");
    if (candidate.expect_result_markers !== undefined) {
      const markers = candidate.expect_result_markers;
      if (control) throw new Error("Result markers apply only to successful calls");
      if (!Array.isArray(markers) || markers.length < 1 || markers.length > MAX_RESULT_MARKERS ||
          markers.some((marker) => typeof marker !== "string" || !marker.trim() || marker.length > 120)) throw new Error(`expect_result_markers must list one to ${MAX_RESULT_MARKERS} short strings`);
    }
    // A discovery control never reaches a tool call, so it carries no call.
    if (candidate.expect_discovery_error) {
      if (candidate.call !== undefined) throw new Error("A discovery-error control makes no tool call; remove its call");
      continue;
    }
    if (!candidate.call || typeof candidate.call.name !== "string" || !candidate.call.arguments || Array.isArray(candidate.call.arguments) || typeof candidate.call.arguments !== "object") throw new Error("Supply a reviewed read-only tool call and synthetic arguments");
  }
}
