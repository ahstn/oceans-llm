import Activity01Icon from "@hugeicons/core-free-icons/Activity01Icon";
import AiCloudIcon from "@hugeicons/core-free-icons/AiCloudIcon";
import ApiIcon from "@hugeicons/core-free-icons/ApiIcon";
import Book02Icon from "@hugeicons/core-free-icons/Book02Icon";
import BookOpen01Icon from "@hugeicons/core-free-icons/BookOpen01Icon";
import Calendar03Icon from "@hugeicons/core-free-icons/Calendar03Icon";
import CloudIcon from "@hugeicons/core-free-icons/CloudIcon";
import CloudServerIcon from "@hugeicons/core-free-icons/CloudServerIcon";
import CodeFolderIcon from "@hugeicons/core-free-icons/CodeFolderIcon";
import ComputerTerminal01Icon from "@hugeicons/core-free-icons/ComputerTerminal01Icon";
import Configuration01Icon from "@hugeicons/core-free-icons/Configuration01Icon";
import DashboardSquare01Icon from "@hugeicons/core-free-icons/DashboardSquare01Icon";
import Database01Icon from "@hugeicons/core-free-icons/Database01Icon";
import DocumentCodeIcon from "@hugeicons/core-free-icons/DocumentCodeIcon";
import FileSearchIcon from "@hugeicons/core-free-icons/FileSearchIcon";
import GithubIcon from "@hugeicons/core-free-icons/GithubIcon";
import Image01Icon from "@hugeicons/core-free-icons/Image01Icon";
import Invoice03Icon from "@hugeicons/core-free-icons/Invoice03Icon";
import Key01Icon from "@hugeicons/core-free-icons/Key01Icon";
import McpServerIcon from "@hugeicons/core-free-icons/McpServerIcon";
import RoboticIcon from "@hugeicons/core-free-icons/RoboticIcon";
import Rocket01Icon from "@hugeicons/core-free-icons/Rocket01Icon";
import Route01Icon from "@hugeicons/core-free-icons/Route01Icon";
import Router01Icon from "@hugeicons/core-free-icons/Router01Icon";
import SaveMoneyDollarIcon from "@hugeicons/core-free-icons/SaveMoneyDollarIcon";
import ServerStack01Icon from "@hugeicons/core-free-icons/ServerStack01Icon";
import Shield01Icon from "@hugeicons/core-free-icons/Shield01Icon";
import Tag01Icon from "@hugeicons/core-free-icons/Tag01Icon";
import TestTube01Icon from "@hugeicons/core-free-icons/TestTube01Icon";
import UserLock01Icon from "@hugeicons/core-free-icons/UserLock01Icon";
import WorkflowSquare01Icon from "@hugeicons/core-free-icons/WorkflowSquare01Icon";

type HugeIconNode = readonly [
  string,
  Readonly<Record<string, string | number | boolean>>,
];

const sidebarIcons: Record<string, readonly HugeIconNode[]> = {
  "/getting-started": BookOpen01Icon,
  "/overview-features": Rocket01Icon,
  "/setup/runtime-bootstrap-and-access": Rocket01Icon,
  "/setup/deploy-and-operations": CloudServerIcon,
  "/mcp/mcp-client-setup": ComputerTerminal01Icon,
  "/setup/kubernetes-and-helm": ServerStack01Icon,
  "/configuration/configuration-reference": Configuration01Icon,
  "/configuration/model-routing-and-api-behavior": Route01Icon,
  "/configuration/client-harness-configuration": ComputerTerminal01Icon,
  "/configuration/pricing-catalog-and-accounting": Invoice03Icon,
  "/configuration/mcp-servers": McpServerIcon,
  "/providers/openrouter": Router01Icon,
  "/providers/aws-bedrock": CloudIcon,
  "/providers/gcp-cloud-run-openai-compat": CloudServerIcon,
  "/providers/gcp-vertex": AiCloudIcon,
  "/mcp/mcp-tool-access": Key01Icon,
  "/mcp/mcp-invocations": Activity01Icon,
  "/operations/tagging": Tag01Icon,
  "/operations/observability-and-request-logs": FileSearchIcon,
  "/operations/observability/export-traces-and-metrics": Activity01Icon,
  "/operations/observability/request-logs": FileSearchIcon,
  "/operations/agent-harness-usage": RoboticIcon,
  "/operations/gateway-guardrails": Shield01Icon,
  "/operations/operator-runbooks": Book02Icon,
  "/access/identity-and-access": UserLock01Icon,
  "/access/service-accounts": Key01Icon,
  "/access/budgets": SaveMoneyDollarIcon,
  "/access/oidc-and-sso-status": Shield01Icon,
  "/access/google-oauth-admin-setup": AiCloudIcon,
  "/access/github-oauth-admin-setup": GithubIcon,
  "/access/admin-control-plane": DashboardSquare01Icon,
  "/reference/request-lifecycle-and-failure-modes": WorkflowSquare01Icon,
  "/reference/provider-api-compatibility": ApiIcon,
  "/contributing": Book02Icon,
  "/contributing/development/authentication-testing": TestTube01Icon,
  "/contributing/mcp/mcp-registry-and-discovery": Database01Icon,
  "/contributing/operations/budgets-and-spending": SaveMoneyDollarIcon,
  "/contributing/reference/data-relationships": Database01Icon,
  "/contributing/reference/admin-api-contract-workflow": DocumentCodeIcon,
  "/contributing/reference/migration-authoring": CodeFolderIcon,
  "/contributing/reference/e2e-contract-tests": TestTube01Icon,
  "/contributing/reference/screenshots": Image01Icon,
  "/contributing/reference/release-process": Calendar03Icon,
  "/contributing/implementation-plans/2026-08-05-configurable-admin-page-permissions": CodeFolderIcon,
  "/contributing/implementation-plans/issue-206-service-account-config": CodeFolderIcon,
  "/contributing/interviews/2026-04-24-request-id-and-request-attempt-observability": FileSearchIcon,
  "/contributing/interviews/2026-05-11-budget-hierarchy-owner-taxonomy": FileSearchIcon,
  "/contributing/interviews/2026-05-27-mcp-gateway-auth-alignment": FileSearchIcon,
  "/contributing/superpowers/specs/2026-04-24-request-id-and-request-attempt-observability-design": DocumentCodeIcon,
};

export function installSidebarIcons() {
  if (typeof document === "undefined" || document.getElementById("docs-sidebar-icons")) {
    return;
  }

  const style = document.createElement("style");
  style.id = "docs-sidebar-icons";
  style.textContent = Object.entries(sidebarIcons)
    .map(
      ([href, icon]) =>
        `.VPSidebarItem .link[href*="${href}"]::before{--docs-sidebar-icon:url("${iconToDataUri(icon)}")}`,
    )
    .join("\n");
  document.head.append(style);
}

function iconToDataUri(icon: readonly HugeIconNode[]) {
  const children = icon
    .map(([tag, attrs]) => {
      const attrText = Object.entries(attrs)
        .filter(([key]) => key !== "key")
        .map(([key, value]) => `${camelToKebab(key)}='${String(value)}'`)
        .join(" ");
      return `<${tag} ${attrText}/>`;
    })
    .join("");
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none'>${children}</svg>`;
  return `data:image/svg+xml,${encodeSvg(svg)}`;
}

function camelToKebab(value: string) {
  return value.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);
}

function encodeSvg(svg: string) {
  return svg
    .replace(/\s+/g, " ")
    .replace(/#/g, "%23")
    .replace(/</g, "%3C")
    .replace(/>/g, "%3E")
    .replace(/"/g, "'");
}
