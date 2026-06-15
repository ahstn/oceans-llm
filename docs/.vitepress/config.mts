import { defineConfig } from "vitepress";

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
      {
        text: "Reference",
        link: "/reference/request-lifecycle-and-failure-modes",
      },
    ],
    sidebar: [
      {
        text: "Getting Started",
        items: [{ text: "Documentation Index", link: "/getting-started" }],
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
          { text: "MCP Client Setup", link: "/setup/mcp-client-setup" },
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
          { text: "MCP Servers", link: "/configuration/mcp-servers" },
        ],
      },
      {
        text: "Providers",
        items: [
          { text: "OpenRouter", link: "/providers/openrouter" },
          { text: "AWS Bedrock", link: "/providers/aws-bedrock" },
          {
            text: "Google Cloud Run OpenAI-Compatible",
            link: "/providers/gcp-cloud-run-openai-compat",
          },
          { text: "Google Vertex AI", link: "/providers/gcp-vertex" },
        ],
      },
      {
        text: "Operations",
        items: [
          {
            text: "Budgets and Spending",
            link: "/operations/budgets-and-spending",
          },
          { text: "Tagging", link: "/operations/tagging" },
          {
            text: "Observability and Request Logs",
            link: "/operations/observability-and-request-logs",
            items: [
              {
                text: "Request Logs",
                link: "/operations/observability/request-logs",
              },
              {
                text: "MCP Invocations",
                link: "/operations/observability/mcp-invocations",
              },
              {
                text: "MCP Registry and Discovery",
                link: "/operations/observability/mcp-registry-and-discovery",
              },
            ],
          },
          {
            text: "Agent Harness Usage",
            link: "/operations/agent-harness-usage",
          },
          { text: "Admin Runbooks", link: "/operations/operator-runbooks" },
        ],
      },
      {
        text: "Access",
        items: [
          { text: "Identity and Access", link: "/access/identity-and-access" },
          { text: "Service Accounts", link: "/access/service-accounts" },
          { text: "MCP Tool Access", link: "/access/mcp-tool-access" },
          { text: "Budgets", link: "/access/budgets" },
          { text: "OIDC and SSO", link: "/access/oidc-and-sso-status" },
          {
            text: "GitHub OAuth SSO Setup",
            link: "/access/github-oauth-admin-setup",
          },
          { text: "Admin Control Plane", link: "/access/admin-control-plane" },
        ],
      },
      {
        text: "Development",
        items: [
          {
            text: "Testing Authentication Locally",
            link: "/development/authentication-testing",
          },
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
          { text: "Data Relationships", link: "/reference/data-relationships" },
          {
            text: "Admin API Contract Workflow",
            link: "/reference/admin-api-contract-workflow",
          },
          {
            text: "Migration Authoring",
            link: "/reference/migration-authoring",
          },
          {
            text: "End-to-End Contract Tests",
            link: "/reference/e2e-contract-tests",
          },
          { text: "Screenshots", link: "/reference/screenshots" },
          { text: "Release Process", link: "/reference/release-process" },
        ],
      },
    ],
    outline: {
      level: [2, 3],
    },
  },
});
