/**
 * Leading sidebar icons, keyed by page path. The content schema has no
 * per-page icon field, so the sidebar looks each link up here. A page that
 * isn't listed renders without an icon. Names are Phosphor icons (`ph:`).
 */
const sidebarIcons: Record<string, string> = {
  "/": "ph:book-open",
  "/features": "ph:rocket-launch",
  "/setup": "ph:compass",
  "/setup/first-access": "ph:sign-in",
  "/setup/deploy-and-operations": "ph:tree-structure",
  "/setup/kubernetes-and-helm": "ph:stack",
  "/setup/self-hosting": "ph:hard-drives",
  "/configuration": "ph:sliders-horizontal",
  "/configuration/model-routing": "ph:path",
  "/configuration/models-and-routes": "ph:cube",
  "/configuration/providers": "ph:plugs-connected",
  "/configuration/client-harnesses": "ph:terminal-window",
  "/providers/openrouter": "ph:shuffle",
  "/providers/typesafe": "ph:list-checks",
  "/providers/opencode-zen": "ph:code-block",
  "/providers/aws-bedrock": "ph:cloud",
  "/providers/aws-bedrock/openai": "ph:open-ai-logo",
  "/providers/github-copilot": "ph:github-logo",
  "/providers/gcp-vertex": "ph:google-logo",
  "/providers/gcp-cloud-run": "ph:cloud-arrow-up",
  "/access/identity-and-access": "ph:users-three",
  "/access/service-accounts": "ph:robot",
  "/access/sso": "ph:identification-badge",
  "/access/sso/google": "ph:google-logo",
  "/access/sso/github": "ph:github-logo",
  "/access/admin-control-plane": "ph:layout",
  "/access/admin-audit-log": "ph:clipboard-text",
  "/access/access-settings": "ph:lock-key",
  "/spend/budgets": "ph:wallet",
  "/spend/budget-settings": "ph:piggy-bank",
  "/spend/rate-limits": "ph:gauge",
  "/spend/pricing-and-accounting": "ph:receipt",
  "/extensions/mcp-servers": "ph:plug",
  "/extensions/mcp-client-setup": "ph:terminal",
  "/extensions/mcp-tool-access": "ph:key",
  "/extensions/mcp-invocations": "ph:list-magnifying-glass",
  "/extensions/skills": "ph:sparkle",
  "/observability": "ph:chart-line",
  "/observability/request-logs": "ph:file-magnifying-glass",
  "/observability/export-traces-and-metrics": "ph:export",
  "/observability/agent-harness-usage": "ph:chart-bar",
  "/observability/agent-session-analysis": "ph:user-focus",
  "/observability/tagging": "ph:tag",
  "/operations/gateway-guardrails": "ph:shield-check",
  "/operations/admin-runbooks": "ph:book-bookmark",
  "/reference/provider-api-compatibility": "ph:brackets-curly",
  "/reference/request-lifecycle": "ph:flow-arrow",
};

export function sidebarIcon(href: string): string | undefined {
  const path = href.length > 1 ? href.replace(/\/+$/, "") : href;
  return sidebarIcons[path];
}
