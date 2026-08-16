import { defineConfig } from "vitepress";

const primarySidebar = [
  {
    text: "Getting Started",
    items: [
      { text: "Documentation Index", link: "/getting-started" },
      { text: "Overview and Features", link: "/overview-features" }
    ],
  },
  {
    text: "Setup",
    items: [
      {
        text: "Runtime Bootstrap and Access",
        link: "/setup/runtime-bootstrap-and-access",
      },
      {
        text: "Deploy and Operations",
        link: "/setup/deploy-and-operations",
      },
      { text: "Kubernetes and Helm", link: "/setup/kubernetes-and-helm" },
    ],
  },
  {
    text: "Configuration",
    items: [
      {
        text: "Configuration Reference",
        link: "/configuration/configuration-reference",
      },
      {
        text: "Model Routing and API Behavior",
        link: "/configuration/model-routing-and-api-behavior",
      },
      {
        text: "Client Harness Configuration",
        link: "/configuration/client-harness-configuration",
      },
      {
        text: "Pricing Catalog and Accounting",
        link: "/configuration/pricing-catalog-and-accounting",
      },
    ],
  },
  {
    text: "Providers",
    items: [
      { text: "OpenRouter", link: "/providers/openrouter" },
      {
        text: "AWS Bedrock",
        link: "/providers/aws-bedrock",
        items: [
          { text: "OpenAI Models", link: "/providers/aws-bedrock-openai-gpt-55" },
        ],
      },
      {
        text: "Google Cloud Run",
        link: "/providers/gcp-cloud-run-openai-compat",
      },
      { text: "Google Vertex AI", link: "/providers/gcp-vertex" },
    ],
  },
  {
    text: "MCP",
    items: [
      { text: "Client Setup", link: "/mcp/mcp-client-setup" },
      { text: "MCP Servers", link: "/configuration/mcp-servers" },
      { text: "MCP Tool Access", link: "/mcp/mcp-tool-access" },
      { text: "Invocation Logs", link: "/mcp/mcp-invocations" },
    ],
  },
  {
    text: "Operations",
    items: [
      { text: "Tagging", link: "/operations/tagging" },
      {
        text: "Observability and Request Logs",
        link: "/operations/observability-and-request-logs",
        items: [
          {
            text: "Export Traces and Metrics",
            link: "/operations/observability/export-traces-and-metrics",
          },
          {
            text: "Request Logs",
            link: "/operations/observability/request-logs",
          },
        ],
      },
      {
        text: "Agent Session Analysis",
        link: "/operations/agent-session-analysis",
      },
      {
        text: "Agent Harness Usage",
        link: "/operations/agent-harness-usage",
      },
      { text: "Admin Runbooks", link: "/operations/operator-runbooks" },
      {
        text: "GitHub Copilot Installation-Token Canary",
        link: "/operations/github-copilot-installation-canary",
      },
    ],
  },
  {
    text: "Access",
    items: [
      { text: "Identity and Access", link: "/access/identity-and-access" },
      { text: "Service Accounts", link: "/access/service-accounts" },
      { text: "Budgets", link: "/access/budgets" },
      {
        text: "OIDC and SSO",
        link: "/access/oidc-and-sso-status",
        items: [
          {
            text: "Google OAuth 2.0 / OIDC SSO Setup",
            link: "/access/google-oauth-admin-setup",
          },
          {
            text: "GitHub OAuth SSO Setup",
            link: "/access/github-oauth-admin-setup",
          },
        ],
      },
      { text: "Admin Control Plane", link: "/access/admin-control-plane" },
    ],
  },
  {
    text: "Reference",
    items: [
      {
        text: "Request Lifecycle and Failure Modes",
        link: "/reference/request-lifecycle-and-failure-modes",
      },
      {
        text: "Provider API Compatibility",
        link: "/reference/provider-api-compatibility",
      },
    ],
  },
];

const contributingSidebar = [
  {
    text: "Contributing & Internal",
    items: [{ text: "Contributor Index", link: "/contributing/" }],
  },
  {
    text: "Maintainer Workflows",
    items: [
      { text: "Release Process", link: "/contributing/reference/release-process" },
      {
        text: "Migration Authoring",
        link: "/contributing/reference/migration-authoring",
      },
      {
        text: "Testing Authentication Locally",
        link: "/contributing/development/authentication-testing",
      },
      {
        text: "MCP Registry and Discovery",
        link: "/contributing/mcp/mcp-registry-and-discovery",
      },
      { text: "Screenshots", link: "/contributing/reference/screenshots" },
    ],
  },
  {
    text: "Contracts and Tests",
    items: [
      {
        text: "Agent Session Analysis Architecture",
        link: "/contributing/reference/agent-session-analysis",
      },
      {
        text: "Admin API Contract Workflow",
        link: "/contributing/reference/admin-api-contract-workflow",
      },
      {
        text: "End-to-End Contract Tests",
        link: "/contributing/reference/e2e-contract-tests",
      },
    ],
  },
  {
    text: "Data and Accounting",
    items: [
      {
        text: "Data Relationships",
        link: "/contributing/reference/data-relationships",
      },
      {
        text: "Budgets and Spending",
        link: "/contributing/operations/budgets-and-spending",
      },
    ],
  },
  {
    text: "Plans and Research",
    items: [
      {
        text: "Configurable Admin Page Permissions",
        link: "/contributing/implementation-plans/2026-08-05-configurable-admin-page-permissions",
      },
      {
        text: "Issue 206 Service Account Config",
        link: "/contributing/implementation-plans/issue-206-service-account-config",
      },
      {
        text: "Request Attempt Observability Interview",
        link: "/contributing/interviews/2026-04-24-request-id-and-request-attempt-observability",
      },
      {
        text: "Budget Owner Taxonomy Interview",
        link: "/contributing/interviews/2026-05-11-budget-hierarchy-owner-taxonomy",
      },
      {
        text: "MCP Gateway Auth Alignment Interview",
        link: "/contributing/interviews/2026-05-27-mcp-gateway-auth-alignment",
      },
      {
        text: "Request Attempt Observability Design",
        link: "/contributing/superpowers/specs/2026-04-24-request-id-and-request-attempt-observability-design",
      },
    ],
  },
];

export default defineConfig({
  title: "Oceans LLM Docs",
  description: "Operator and maintainer docs for the Oceans LLM gateway.",
  lang: "en-US",
  appearance: "dark",
  head: [
    [
      "link",
      {
        rel: "icon",
        type: "image/png",
        href: "/images/oceans-logo-rounded-square.png",
      },
    ],
  ],
  ignoreDeadLinks: true,
  srcExclude: ["README.md", "AGENTS.md", "adr/**", "internal/**"],
  themeConfig: {
    logo: {
      src: "/images/oceans-logo-rounded-square.png",
      alt: "Oceans LLM",
    },
    siteTitle: "Oceans LLM",
    search: {
      provider: "local",
    },
    nav: [
      { text: "Home", link: "/" },
      { text: "Getting Started", link: "/getting-started" },
      { text: "Setup", link: "/setup/runtime-bootstrap-and-access" },
      { text: "Contributing & Internal", link: "/contributing/" },
    ],
    sidebar: {
      "/contributing/": contributingSidebar,
      "/": primarySidebar,
    },
    outline: {
      level: [2, 3],
    },
  },
});
