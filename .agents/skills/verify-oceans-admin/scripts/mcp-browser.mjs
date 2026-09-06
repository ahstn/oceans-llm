// Real UI mutations with production API reads as the saved-state authority.
const authLabels = { none: "None", gateway_static_header: "Gateway static header", gateway_bearer_token: "Gateway bearer token" };

export async function registerServer(ui, candidate) {
  const { page, baseURL, expect, adminJson, poll, owned, actions, capture, runId } = ui;
  const serverKey = `verify-${runId.toLowerCase().replace(/[^a-z0-9-]/g, "-").slice(-30)}-${candidate.key}`;
  const displayName = `Verification ${candidate.label} ${runId.slice(-8)}`;
  if ((await adminJson(page, "/api/v1/admin/mcp/servers?include_disabled=true")).items.some((item) => item.server_key === serverKey)) throw new Error("Verification server key already exists; use a new run ID");
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

export async function verifyWorkbench(ui, candidates) {
  const { page, baseURL, expect, adminJson, poll, actions, capture, config } = ui;
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
  if (config.expected_gateway_url && info.endpoint !== config.expected_gateway_url) throw new Error("Generated MCP endpoint differs from configured public gateway URL");
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
  const { page, baseURL, expect, adminJson, poll, owned, runId, actions, capture } = ui;
  await page.goto(`${baseURL}/admin/api-keys`, { waitUntil: "networkidle" });
  const catalog = await adminJson(page, "/api/v1/admin/api-keys");
  const owner = catalog.users.find((item) => item.email === "admin@local") ?? catalog.users[0];
  const model = catalog.models.find((item) => item.key === "deepseek-v4-flash-0731") ?? catalog.models[0];
  if (!owner || !model) throw new Error("No owner or constrained model is available for temporary key");
  const keyName = `MCP verification ${runId}`;
  if (catalog.items.some((item) => item.name === keyName)) throw new Error("Verification API key name already exists; use a new run ID");
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
  const { page, adminJson, poll, owned, runId } = ui;
  const toolsetKey = `verify-${runId.toLowerCase().replace(/[^a-z0-9-]/g, "-").slice(-30)}-${suffix}`;
  const name = `MCP verification ${suffix} ${runId.slice(-8)}`;
  if ((await adminJson(page, "/api/v1/admin/mcp/toolsets?include_disabled=true")).items.some((item) => item.toolset_key === toolsetKey)) throw new Error("Verification tool set key already exists; use a new run ID");
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
