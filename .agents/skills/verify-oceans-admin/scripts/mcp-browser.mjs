// Real UI mutations with production API reads as the saved-state authority.
import fs from "node:fs/promises";

const authLabels = { none: "None", gateway_static_header: "Gateway static credential", gateway_bearer_token: "Gateway bearer token" };

/**
 * Names this driver attempt's records. Cleanup disables rather than deletes them, so a retry
 * in the same run needs keys of its own. `key` stays within 30 URL-safe characters.
 */
export function attemptScope(runId, now = Date.now()) {
  const attempt = now.toString(36).slice(-5);
  return {
    key: `${runId.toLowerCase().replace(/[^a-z0-9-]/g, "-").slice(-24)}-${attempt}`,
    label: `${runId.slice(-8)} ${attempt}`,
  };
}

/** The aggregate MCP endpoint the gateway generates for a client base URL (see `normalize_gateway_base_url`). */
export function mcpEndpointFor(baseUrl) {
  const trimmed = baseUrl.replace(/\/+$/, "");
  return `${trimmed.endsWith("/v1") ? trimmed.slice(0, -3) : trimmed}/mcp`;
}

/**
 * Reads what `launch` resolved for the gateway: which OCEANS_MCP_DISCOVERY_* aliases were set
 * and the client configuration base URL. Without a launch record, falls back to this process.
 */
export async function loadStackEnv(file, env = process.env) {
  if (file) {
    try {
      const saved = JSON.parse(await fs.readFile(file, "utf8"));
      return { source: "launch", aliases: new Set(saved.mcpDiscoveryAliases), clientConfigBaseUrl: saved.clientConfigBaseUrl ?? null };
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  const aliases = Object.keys(env).filter((name) => name.startsWith("OCEANS_MCP_DISCOVERY_") && env[name]);
  return { source: "driver", aliases: new Set(aliases), clientConfigBaseUrl: env.GATEWAY_CLIENT_CONFIG_BASE_URL ?? null };
}

export async function registerServer(ui, candidate) {
  const { page, expect, adminJson, poll, actions, capture } = ui;
  const { server, displayName, manage } = await addServer(ui, candidate);
  await manage.getByRole("button", { name: `Refresh ${displayName}`, exact: true }).click();
  const discovered = await poll(async () => {
    const current = (await adminJson(page, "/api/v1/admin/mcp/servers")).items.find((item) => item.id === server.id);
    return current?.last_discovery_at ? current : null;
  }, "completed MCP discovery");
  if (discovered.last_discovery_status !== "success") throw new Error("MCP discovery did not succeed");
  await manage.getByRole("button", { name: "Tools", exact: true }).filter({ visible: true }).click();
  const tools = (await adminJson(page, `/api/v1/admin/mcp/servers/${server.id}/tools?include_inactive=true`)).items;
  const panel = manage.getByTestId("mcp-server-tools");
  await expect(panel.getByRole("checkbox")).toHaveCount(tools.length);
  const tool = tools.find((item) => item.upstream_name === candidate.call.name && item.is_active);
  if (!tool) throw new Error("Reviewed read-only call is missing from discovered tools");
  await expect(panel.getByRole("checkbox", { name: `Select ${tool.display_name}`, exact: true })).toBeVisible();
  await capture(page, `03-${candidate.key}-discovery`);
  await manage.getByRole("button", { name: "Close", exact: true }).click();
  const row = page.getByTestId("mcp-server-list").getByRole("row").filter({ has: page.getByRole("button", { name: `Open ${displayName}`, exact: true }) });
  await expect(row.getByTitle("Tool count from the last successful discovery")).toHaveText(String(tools.filter((item) => item.is_active).length));
  actions.push({ action: `Register and discover ${candidate.key} through Registry`, result: `${tools.length} tools matched the API and registry count` });
  return { ...candidate, server: discovered, tool, toolCount: tools.length };
}

// `.invalid` never resolves (RFC 6761), so this discovery fails without contacting a real host.
// The overview explains the failure once, with Retry and no toast; a registry refresh, with the
// overview closed, toasts briefly and links back to it.
export async function verifyDiscoveryFailure(ui) {
  const { page, expect, adminJson, poll, actions, capture } = ui;
  const candidate = { key: "unreachable", label: "Unreachable", server_url: "https://unreachable.invalid/mcp", auth_mode: "none", auth_config: {} };
  const registered = await addServer(ui, candidate);
  const { server, displayName, manage } = registered;
  const explanation = "Couldn't reach unreachable.invalid. Check the endpoint, then Retry.";
  const discoveryToasts = page.locator("[data-sonner-toast]").filter({ hasText: /discovery|transport/i });
  await manage.getByRole("button", { name: `Refresh ${displayName}`, exact: true }).click();
  const failed = await poll(async () => {
    const current = (await adminJson(page, "/api/v1/admin/mcp/servers")).items.find((item) => item.id === server.id);
    return current?.last_discovery_at ? current : null;
  }, "failed MCP discovery");
  if (failed.last_discovery_status !== "failed" || !failed.last_error_summary?.startsWith("MCP transport failure")) throw new Error("Unreachable discovery did not fail in transport");
  const alert = manage.getByRole("alert");
  await expect(alert).toContainText("Discovery failed");
  await expect(alert).toContainText(explanation);
  await expect(alert).toContainText(failed.last_error_summary);
  await expect(alert.getByRole("button", { name: "Retry", exact: true })).toBeEnabled();
  // ServersTab calls toast() in the same tick that renders this alert, and Sonner mounts a toast one
  // task later, so a task and a frame are enough to see one. Count once: a retrying assertion would
  // pass after a toast had come and gone, since Sonner dismisses toasts after 4 s.
  await page.evaluate(() => new Promise((resolve) => setTimeout(() => requestAnimationFrame(() => resolve()), 0)));
  expect(await discoveryToasts.count(), "discovery toasts beside the overview alert").toBe(0);
  await capture(page, "03-unreachable-overview");
  for (const [name, label] of [[`Refresh ${displayName}`, "Refresh tools"], [`Edit ${displayName}`, "Edit configuration"], [`More actions for ${displayName}`, "More actions"], ["Close", "Close"]]) {
    // Radix opens a tooltip on pointer movement into its trigger, and a diagonal path would cross the
    // neighbouring header buttons. Approach each one straight up from below the header instead.
    const box = await manage.getByRole("button", { name, exact: true }).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height + 80, { steps: 4 });
    // A trigger opens only once the previous tooltip has closed and released its hover grace area.
    await expect(page.locator('[data-slot="tooltip-content"]')).toHaveCount(0);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 8 });
    await expect(page.locator('[data-slot="tooltip-content"]').filter({ hasText: label })).toBeVisible();
  }
  await manage.getByRole("button", { name: "Close", exact: true }).click();
  const row = page.getByTestId("mcp-server-list").getByRole("row").filter({ has: page.getByRole("button", { name: `Open ${displayName}`, exact: true }) });
  await row.getByRole("button", { name: `Refresh ${displayName}`, exact: true }).click();
  const toast = discoveryToasts.filter({ hasText: `Discovery failed for ${displayName}` });
  await expect(toast).toHaveCount(1);
  await toast.hover(); // Sonner pauses dismissal while hovered.
  await capture(page, "03-unreachable-toast");
  await toast.getByRole("button", { name: "View", exact: true }).click();
  await expect(manage.getByRole("alert")).toContainText(explanation);
  actions.push({ action: "Fail discovery for an unreachable server", result: "Overview explained it once with Retry and no toast; a registry refresh toasted with View" });
  return registered;
}

// Disabling lives in the header's More actions menu, not beside Close.
export async function disableFromMenu(ui, { server, displayName, manage }) {
  const { page, expect, adminJson, poll, actions, capture } = ui;
  await expect(manage.getByRole("button", { name: `Disable ${displayName}`, exact: true })).toHaveCount(0);
  await manage.getByRole("button", { name: `More actions for ${displayName}`, exact: true }).click();
  await page.getByRole("menuitem", { name: "Disable server", exact: true }).click();
  await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/servers?include_disabled=true")).items.find((item) => item.id === server.id && item.status === "disabled"), "disabled MCP server");
  await expect(manage.getByText("Disabled", { exact: true })).toBeVisible();
  await expect(manage.getByRole("button", { name: `Refresh ${displayName}`, exact: true })).toBeDisabled();
  await capture(page, "03-unreachable-disabled");
  actions.push({ action: "Disable the unreachable server from More actions", result: "API reported it disabled; Refresh is unavailable" });
}

// The same menu brings a disabled server back. Discovery reads as not run until it is refreshed.
export async function enableFromMenu(ui, { server, displayName, manage }) {
  const { page, expect, adminJson, poll, actions, capture } = ui;
  await manage.getByRole("button", { name: `More actions for ${displayName}`, exact: true }).click();
  await expect(page.getByRole("menuitem", { name: "Disable server", exact: true })).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Enable server", exact: true }).click();
  const enabled = await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/servers")).items.find((item) => item.id === server.id && item.status === "active"), "enabled MCP server");
  if (enabled.disabled_at || enabled.last_discovery_status) throw new Error("Enabled server kept its disabled state");
  await expect(manage.getByText("Active", { exact: true })).toBeVisible();
  await expect(manage.getByText("Not discovered", { exact: true })).toBeVisible();
  await expect(manage.getByRole("button", { name: `Refresh ${displayName}`, exact: true })).toBeEnabled();
  await capture(page, "03-unreachable-enabled");
  await manage.getByRole("button", { name: "Close", exact: true }).click();
  actions.push({ action: "Enable the unreachable server from More actions", result: "API reported it active with discovery not run" });
}

// Registers through `Add server`, then waits for the `Manage MCP server` dialog that follows.
async function addServer(ui, candidate) {
  const { page, baseURL, expect, adminJson, poll, owned, scope } = ui;
  const serverKey = `verify-${scope.key}-${candidate.key}`;
  const displayName = `Verification ${candidate.label} ${scope.label}`;
  if ((await adminJson(page, "/api/v1/admin/mcp/servers?include_disabled=true")).items.some((item) => item.server_key === serverKey)) throw new Error("Verification server key already exists; rerun the driver for a new attempt scope");
  await page.goto(`${baseURL}/admin/mcp?tab=servers`, { waitUntil: "networkidle" });
  const dialog = page.getByRole("dialog", { name: "Add MCP server", exact: true });
  await expect(async () => {
    if (!await dialog.isVisible()) await page.getByRole("button", { name: "Add server", exact: true }).click();
    await expect(dialog).toBeVisible({ timeout: 1000 });
  }).toPass({ timeout: 15000, intervals: [500] });
  await dialog.getByLabel("Server key", { exact: true }).fill(serverKey);
  await dialog.getByLabel("Display name", { exact: true }).fill(displayName);
  await dialog.getByLabel("Description", { exact: true }).fill("Temporary bounded MCP verification");
  await dialog.getByLabel("Server URL", { exact: true }).fill(candidate.server_url);
  await dialog.getByRole("combobox").click();
  await page.getByRole("option", { name: authLabels[candidate.auth_mode], exact: true }).click();
  await dialog.getByLabel("Auth config JSON", { exact: true }).fill(JSON.stringify(candidate.auth_config ?? {}));
  await dialog.getByLabel("Timeout ms", { exact: true }).fill("30000");
  const ownedServer = { server_key: serverKey };
  owned.servers.push(ownedServer);
  await dialog.getByRole("button", { name: "Add server", exact: true }).click();
  const server = await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/servers")).items.find((item) => item.server_key === serverKey), "created MCP server");
  Object.assign(ownedServer, server);
  const manage = page.getByRole("dialog", { name: "Manage MCP server", exact: true });
  await manage.waitFor();
  return { server, displayName, manage };
}

export async function verifyWorkbench(ui, candidates) {
  const { page, baseURL, expect, adminJson, poll, actions, capture, expectedGatewayUrl } = ui;
  await page.goto(`${baseURL}/admin/mcp/toolsets`, { waitUntil: "networkidle" });
  const primary = await createSet(ui, "live");
  for (const candidate of candidates) await toolCheckbox(page, candidate.tool).check();
  await expect(setRow(page, primary).getByRole("status")).toContainText(`${candidates.length} tool`);
  await expect(setRow(page, primary).getByRole("status")).toContainText("Unsaved");
  await page.getByRole("button", { name: `Save ${primary.display_name}`, exact: true }).click();
  const expectedIds = candidates.map((candidate) => candidate.tool.id).sort();
  await poll(async () => sameIds((await members(ui, primary)).tool_ids, expectedIds), "saved tool set membership");
  await page.reload({ waitUntil: "networkidle" });
  await selectSet(page, primary);
  for (const candidate of candidates) await expect(toolCheckbox(page, candidate.tool)).toBeChecked();
  const disabledSave = setRow(page, primary).getByRole("button", { name: `Save ${primary.display_name}`, exact: true });
  await expect(disabledSave).toBeDisabled();
  await setRow(page, primary).getByLabel(`Save ${primary.display_name} unavailable`, { exact: true }).hover();
  await expect(page.getByRole("tooltip")).toContainText("Select or change tools to save changes");
  await page.mouse.move(20, 20);
  await capture(page, "04-mcp-workbench-saved");

  const secondary = await createSet(ui, "draft");
  const first = candidates[0].tool;
  await toolCheckbox(page, first).check();
  await selectSet(page, primary);
  await toolCheckbox(page, first).uncheck();
  await selectSet(page, secondary);
  await expect(toolCheckbox(page, first)).toBeChecked();
  await selectSet(page, primary);
  await expect(toolCheckbox(page, first)).not.toBeChecked();
  if (!sameIds((await members(ui, primary)).tool_ids, expectedIds) || (await members(ui, secondary)).tool_ids.length !== 0) throw new Error("Unsaved membership leaked into persistence");
  await capture(page, "05-mcp-workbench-independent-drafts");
  await toolCheckbox(page, first).check();
  await selectSet(page, secondary);
  await toolCheckbox(page, first).uncheck();
  await toolCheckbox(page, first).check();
  await page.getByRole("button", { name: `Save ${secondary.display_name}`, exact: true }).click();
  await poll(async () => sameIds((await members(ui, secondary)).tool_ids, [first.id]), "saved secondary membership");
  await toolCheckbox(page, first).uncheck();
  await page.getByRole("button", { name: `Save ${secondary.display_name}`, exact: true }).click();
  const clear = page.getByRole("alertdialog", { name: "Remove all tools?", exact: true });
  await clear.getByRole("button", { name: "Cancel", exact: true }).click();
  if (!sameIds((await members(ui, secondary)).tool_ids, [first.id])) throw new Error("Cancel changed saved membership");
  await page.getByRole("button", { name: `Save ${secondary.display_name}`, exact: true }).click();
  await clear.getByRole("button", { name: "Save empty tool set", exact: true }).click();
  await poll(async () => (await members(ui, secondary)).tool_ids.length === 0, "saved empty membership");
  await page.reload({ waitUntil: "networkidle" });
  await selectSet(page, secondary);
  await expect(toolCheckbox(page, first)).not.toBeChecked();
  await selectSet(page, primary);
  await page.getByRole("button", { name: `Edit ${primary.display_name}`, exact: true }).click();
  const edit = page.getByRole("dialog", { name: "Edit tool set", exact: true });
  const description = "Verified saved membership and independent drafts";
  await edit.getByLabel("Description", { exact: true }).fill(description);
  await edit.getByRole("button", { name: "Save details", exact: true }).click();
  await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/toolsets")).items.some((item) => item.id === primary.id && item.description === description), "edited tool set description");
  if (!sameIds((await members(ui, primary)).tool_ids, expectedIds)) throw new Error("Metadata edit changed membership");

  await page.getByRole("button", { name: "Connection Info", exact: true }).click();
  const connection = page.getByTestId("toolset-connection-dialog");
  const info = await adminJson(page, "/api/v1/admin/mcp/connection-info");
  if (info.endpoint !== expectedGatewayUrl) throw new Error("Generated MCP endpoint differs from the launched client configuration URL");
  if (!new URL(info.endpoint).pathname.endsWith("/mcp")) throw new Error("Generated endpoint is not aggregate MCP");
  await expect(connection.getByTestId("toolset-connection-panel")).toContainText(info.endpoint);
  for (const client of info.client_configurations) {
    await connection.getByRole("radio", { name: client.label, exact: true }).click();
    await expect(connection.getByRole("heading", { name: `${client.label} setup`, exact: true })).toBeVisible();
    for (const block of client.blocks) {
      const rendered = connection.locator('[data-slot="code-block"]').filter({ has: page.getByText(block.filename, { exact: true }) });
      await expect(rendered).toBeVisible();
      const content = await rendered.locator('[data-slot="code-block-line"]').evaluateAll((lines) =>
        lines.map((line) => line.lastElementChild.textContent.replace(/\u200b/g, "")).join("\n"));
      if (content !== block.content.replace(/\r\n?/g, "\n")) throw new Error("Rendered MCP configuration differs from API");
    }
    await capture(page, `06-mcp-config-${client.key}`);
  }
  await connection.getByRole("button", { name: "Close", exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole("radiogroup", { name: "Choose a tool set", exact: true })).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  if (overflow) throw new Error("Workbench overflows the mobile viewport");
  await capture(page, "07-mcp-workbench-mobile");
  await page.setViewportSize({ width: 1440, height: 1000 });
  actions.push({ action: "Create, save, reload, draft, and edit Workbench sets", result: "Saved members matched API; drafts remained independent; metadata preserved membership" });
  actions.push({ action: "Inspect Connection Info client configurations", result: "Rendered every backend-generated block and verified mobile page fit" });
  return { primary, proof: { toolsetId: primary.id, savedToolIds: expectedIds, independentDrafts: true, savedEmptyMembership: true, metadataPreservesMembership: true, mobileWidth: 390, connectionEndpoint: info.endpoint, generatedClientKeys: info.client_configurations.map((item) => item.key) } };
}

export async function createAccess(ui, primary, candidates, verifyNoMcpGrant) {
  const { page, baseURL, expect, adminJson, poll, owned, scope, actions, capture } = ui;
  await page.goto(`${baseURL}/admin/api-keys`, { waitUntil: "networkidle" });
  const catalog = await adminJson(page, "/api/v1/admin/api-keys");
  const owner = catalog.users.find((item) => item.email === "admin@local") ?? catalog.users[0];
  const model = catalog.models.find((item) => item.key === "deepseek-v4.1-flash") ?? catalog.models[0];
  if (!owner || !model) throw new Error("No owner or constrained model is available for temporary key");
  const keyName = `MCP verification ${scope.key}`;
  if (catalog.items.some((item) => item.name === keyName)) throw new Error("Verification API key name already exists; rerun the driver for a new attempt scope");
  owned.apiKeyName = keyName;
  await page.getByRole("button", { name: "Create API key", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Create API key", exact: true });
  await dialog.getByLabel("Name", { exact: true }).fill(keyName);
  await dialog.getByRole("combobox", { name: "Owner user", exact: true }).click();
  await page.getByRole("option", { name: `${owner.name} (${owner.email})`, exact: true }).click();
  await dialog.getByRole("radio", { name: "Selected models", exact: true }).click();
  await dialog.getByRole("button", { name: "Select models" }).click();
  await page.getByPlaceholder("Search models...", { exact: true }).fill(model.key);
  await page.getByRole("option").filter({ hasText: model.key }).first().click();
  await page.keyboard.press("Escape");
  await dialog.getByRole("button", { name: "Create API key", exact: true }).click();
  const key = await poll(async () => (await adminJson(page, "/api/v1/admin/api-keys")).items.find((item) => item.name === keyName), "temporary gateway key");
  owned.apiKeyId = key.id;
  await page.getByTestId("new-api-key-raw-key").waitFor();
  owned.rawKey = (await page.getByTestId("new-api-key-raw-key").textContent()).trim();
  if (!owned.rawKey.startsWith("gwk_")) throw new Error("One-time gateway secret missing");
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  if (key.model_grant_mode !== "explicit" || !sameIds(key.model_keys, [model.key])) throw new Error("Temporary key is not model constrained");
  const negative = await verifyNoMcpGrant({ page, baseURL, rawKey: owned.rawKey, apiKeyId: owned.apiKeyId, candidates, adminJson, actions });
  await capture(page, "08-mcp-temporary-key");

  await page.goto(`${baseURL}/admin/mcp?tab=access`, { waitUntil: "networkidle" });
  const form = page.locator("form").filter({ has: page.getByRole("button", { name: "Save grant", exact: true }) });
  await form.getByRole("combobox").first().click();
  await page.getByRole("option", { name: "API key", exact: true }).click();
  await form.getByRole("button", { name: "Grant subject", exact: true }).click();
  await page.getByPlaceholder("Search subjects…", { exact: true }).fill(keyName);
  await page.getByRole("option").filter({ hasText: keyName }).click();
  await form.getByRole("button", { name: "Grant target", exact: true }).click();
  await page.getByPlaceholder("Search targets…", { exact: true }).fill(primary.display_name);
  await page.getByRole("option").filter({ hasText: primary.display_name }).click();
  const grant = { subject_kind: "api_key", subject_id: key.id, target_kind: "toolset", target_id: primary.id };
  // Track before click so cleanup can revoke a mutation whose UI response was lost.
  owned.grants.push(grant);
  await form.getByRole("button", { name: "Save grant", exact: true }).click();
  await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/grants")).items.some((item) => item.subject_id === key.id && item.target_id === primary.id && item.is_active), "persisted MCP grant");
  await expect(page.getByTestId("mcp-grant-list")).toContainText(keyName);
  const effective = await adminJson(page, `/api/v1/admin/mcp/effective-access?api_key_id=${key.id}`);
  if (!sameIds(effective.tools.map((item) => item.id), candidates.map((item) => item.tool.id))) throw new Error("Effective access is broader than the selected canary tools");
  await capture(page, "09-mcp-toolset-grant");
  actions.push({ action: "Create constrained key and Tool Set grant through UI", result: "Only reviewed tools are callable; no model request sent" });
  return { proof: { apiKeyId: key.id, modelGrant: model.key, toolsetId: primary.id, effectiveToolCount: effective.tools.length, beforeGrant: negative } };
}

async function createSet(ui, suffix) {
  const { page, adminJson, poll, owned, scope } = ui;
  const toolsetKey = `verify-${scope.key}-${suffix}`;
  const name = `MCP verification ${suffix} ${scope.label}`;
  if ((await adminJson(page, "/api/v1/admin/mcp/toolsets?include_disabled=true")).items.some((item) => item.toolset_key === toolsetKey)) throw new Error("Verification tool set key already exists; rerun the driver for a new attempt scope");
  await page.getByRole("button", { name: "New tool set", exact: true }).first().click();
  const dialog = page.getByRole("dialog", { name: "New tool set", exact: true });
  await dialog.getByLabel("Key", { exact: true }).fill(toolsetKey);
  await dialog.getByLabel("Display name", { exact: true }).fill(name);
  await dialog.getByLabel("Description", { exact: true }).fill("Temporary MCP verification set");
  const ownedSet = { toolset_key: toolsetKey };
  owned.toolsets.push(ownedSet);
  await dialog.getByRole("button", { name: "Create tool set", exact: true }).click();
  const set = await poll(async () => (await adminJson(page, "/api/v1/admin/mcp/toolsets")).items.find((item) => item.toolset_key === toolsetKey), "created tool set");
  Object.assign(ownedSet, set);
  await dialog.waitFor({ state: "hidden" });
  await selectSet(page, set);
  return set;
}

async function selectSet(page, set) {
  await page.getByRole("radio", { name: `Select ${set.display_name}`, exact: true }).click();
  await page.getByTestId("mcp-toolset-detail").getByRole("heading", { name: set.display_name, exact: true }).waitFor();
}

function setRow(page, set) { return page.getByTestId(`toolset-row-${set.id}`); }
function toolCheckbox(page, tool) { return page.locator(`#workbench-tool-${tool.id}`); }
function members(ui, set) { return ui.adminJson(ui.page, `/api/v1/admin/mcp/toolsets/${set.id}/tools`); }
function sameIds(actual, expected) { return JSON.stringify([...actual].sort()) === JSON.stringify([...expected].sort()); }
