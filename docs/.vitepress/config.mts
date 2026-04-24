import { defineConfig } from "vitepress";

export default defineConfig({
  title: "Oceans LLM Docs",
  description: "Operator and maintainer docs for the Oceans LLM gateway.",
  lang: "en-US",
  ignoreDeadLinks: true,
  srcExclude: ["README.md", "adr/**", "internal/**"],
  themeConfig: {
    search: {
      provider: "local",
    },
    nav: [
      { text: "Setup", link: "/setup/runtime-bootstrap-and-access" },
      { text: "Configuration", link: "/configuration/configuration-reference" },
      { text: "Operations", link: "/operations/budgets-and-spending" },
      { text: "Access", link: "/access/identity-and-access" },
      { text: "Reference", link: "/reference/request-lifecycle-and-failure-modes" },
    ],
    sidebar: {
      "/setup/": [
        {
          text: "Setup",
          items: [
            { text: "Runtime Bootstrap and Access", link: "/setup/runtime-bootstrap-and-access" },
            { text: "Deploy and Operations", link: "/setup/deploy-and-operations" },
          ],
        },
      ],
      "/configuration/": [
        {
          text: "Configuration",
          items: [
            { text: "Configuration Reference", link: "/configuration/configuration-reference" },
            { text: "Model Routing and API Behavior", link: "/configuration/model-routing-and-api-behavior" },
            { text: "Pricing Catalog and Accounting", link: "/configuration/pricing-catalog-and-accounting" },
          ],
        },
      ],
      "/operations/": [
        {
          text: "Operations",
          items: [
            { text: "Budgets and Spending", link: "/operations/budgets-and-spending" },
            { text: "Observability and Request Logs", link: "/operations/observability-and-request-logs" },
            { text: "Operator Runbooks", link: "/operations/operator-runbooks" },
          ],
        },
      ],
      "/access/": [
        {
          text: "Access",
          items: [
            { text: "Identity and Access", link: "/access/identity-and-access" },
            { text: "OIDC and SSO Status", link: "/access/oidc-and-sso-status" },
            { text: "Admin Control Plane", link: "/access/admin-control-plane" },
          ],
        },
      ],
      "/reference/": [
        {
          text: "Reference",
          items: [
            { text: "Request Lifecycle and Failure Modes", link: "/reference/request-lifecycle-and-failure-modes" },
            { text: "Provider API Compatibility", link: "/reference/provider-api-compatibility" },
            { text: "Data Relationships", link: "/reference/data-relationships" },
            { text: "Admin API Contract Workflow", link: "/reference/admin-api-contract-workflow" },
            { text: "End-to-End Contract Tests", link: "/reference/e2e-contract-tests" },
            { text: "Release Process", link: "/reference/release-process" },
          ],
        },
      ],
    },
    outline: {
      level: [2, 3],
    },
  },
});
