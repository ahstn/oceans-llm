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
  "/access/access-settings": "ph:lock-key",
  "/access/budget-settings": "ph:piggy-bank",
  "/access/rate-limits": "ph:gauge",
  "/access/pricing-and-accounting": "ph:receipt",
  "/extensions/mcp-servers": "ph:plug",
  "/extensions/skills": "ph:sparkle",
  "/reference/provider-api-compatibility": "ph:brackets-curly",
  "/reference/request-lifecycle": "ph:flow-arrow",
};

export function sidebarIcon(href: string): string | undefined {
  const path = href.length > 1 ? href.replace(/\/+$/, "") : href;
  return sidebarIcons[path];
}
