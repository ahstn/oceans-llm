import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const requireFromAdminUi = createRequire(path.join(repoRoot, "crates/admin-ui/web/package.json"));
const { chromium } = requireFromAdminUi("playwright");

const baseURL = requiredEnv("OCEANS_VERIFY_BASE_URL");
const evidenceDir = requiredEnv("OCEANS_VERIFY_EVIDENCE_DIR");
const gatewayVersion = requiredEnv("OCEANS_VERIFY_GATEWAY_VERSION");
const email = requiredEnv("OCEANS_VERIFY_ADMIN_EMAIL");
const password = requiredEnv("OCEANS_VERIFY_ADMIN_PASSWORD");
const actions = [];
const currencyFormatter = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
});
const numberFormatter = new Intl.NumberFormat("en-US");

await fs.mkdir(evidenceDir, { recursive: true });
const browser = await chromium.launch({ headless: true });

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();
  page.on("console", (message) =>
    console.log(`browser console ${message.type()}: ${message.text()}`),
  );
  page.on("pageerror", (error) => console.log(`browser page error: ${error.message}`));
  page.on("requestfailed", (request) =>
    console.log(
      `browser request failed: ${request.method()} ${request.url()} ${request.failure()?.errorText ?? ""}`,
    ),
  );

  const entryUrl = `${baseURL}/admin/observability/leaderboard`;
  await page.goto(entryUrl, { waitUntil: "domcontentloaded" });
  await waitForSignIn(page);
  actions.push({ action: "open protected leaderboard", result: page.url() });
  await capture(page, "01-observability-login");

  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await Promise.all([
    page.waitForURL(/\/admin\/observability\/leaderboard(?:\?|$)/),
    page.getByRole("button", { name: "Sign in" }).click(),
  ]);
  actions.push({ action: "sign in with seeded platform admin", result: page.url() });

  await page.getByRole("heading", { name: "Leaderboard", exact: true }).waitFor();
  await page.getByTestId("leaderboard-table").waitFor();
  const leaderboard7d = await fetchView(page, "/api/v1/admin/observability/leaderboard?range=7d");
  assertEqual(leaderboard7d.range, "7d", "leaderboard API range");
  const leaderboard7dTable = await assertLeaderboard(page, leaderboard7d);
  await assertChartSeries(page, leaderboard7d.chart_users.length);
  actions.push({
    action: "compare 7d leaderboard with production admin API",
    result: `${leaderboard7d.leaders.length} rendered leaders matched`,
  });
  await capture(page, "02-leaderboard-7d");

  const leaderboard31d = await fetchView(page, "/api/v1/admin/observability/leaderboard?range=31d");
  assertEqual(leaderboard31d.range, "31d", "31d leaderboard API range");
  await selectRange(page, "leaderboard-table", "User", leaderboard31d.leaders, (leader) => ({
    key: leader.user_name,
    requests: leader.total_requests,
  }));
  const leaderboard31dTable = await assertLeaderboard(page, leaderboard31d);
  await assertChartSeries(page, leaderboard31d.chart_users.length);
  actions.push({
    action: "select leaderboard Last 31 days",
    result: `${leaderboard31d.leaders.length} rendered leaders matched`,
  });
  await capture(page, "03-leaderboard-31d");

  const harnessesLink = page.getByRole("link", { name: "Agent Harnesses" }).first();
  await Promise.all([
    page.waitForURL(/\/admin\/observability\/agent-harnesses(?:\?|$)/),
    harnessesLink.click(),
  ]);
  await page.getByRole("heading", { name: "Agent harnesses", exact: true }).waitFor();
  await page.getByTestId("harness-usage-table").waitFor();

  const harness7d = await fetchView(page, "/api/v1/admin/observability/harness-usage?range=7d");
  assertEqual(harness7d.range, "7d", "harness API range");
  const harness7dTable = await assertHarnessUsage(page, harness7d);
  await assertChartSeries(page, harness7d.chart_harnesses.length);
  const iconChecks = await assertHarnessIcons(page, harness7d);
  actions.push({
    action: "compare 7d agent harnesses with production admin API",
    result: `${harness7d.leaders.length} rendered harnesses matched with token values`,
  });
  actions.push({
    action: "inspect Mastra and Oh My Pi rendering",
    result: "Mastra has its harness icon and Oh My Pi is text-only",
  });
  await capture(page, "04-agent-harnesses-7d");

  const harness31d = await fetchView(page, "/api/v1/admin/observability/harness-usage?range=31d");
  assertEqual(harness31d.range, "31d", "31d harness API range");
  await selectRange(page, "harness-usage-table", "Harness", harness31d.leaders, (leader) => ({
    key: leader.agent_harness_label,
    requests: leader.total_requests,
  }));
  const harness31dTable = await assertHarnessUsage(page, harness31d);
  await assertChartSeries(page, harness31d.chart_harnesses.length);
  actions.push({
    action: "select agent harnesses Last 31 days",
    result: `${harness31d.leaders.length} rendered harnesses matched`,
  });
  await capture(page, "05-agent-harnesses-31d");

  const proof = {
    feature: "observability",
    entryUrl,
    finalUrl: page.url(),
    gatewayVersion,
    leaderboard: {
      sevenDays: proofProjection(leaderboard7d, leaderboard7dTable),
      thirtyOneDays: proofProjection(leaderboard31d, leaderboard31dTable),
    },
    harnessUsage: {
      sevenDays: proofProjection(harness7d, harness7dTable),
      thirtyOneDays: proofProjection(harness31d, harness31dTable),
    },
    iconChecks,
    actions,
    generatedAt: new Date().toISOString(),
  };
  await fs.writeFile(
    path.join(evidenceDir, "observability-proof.json"),
    `${JSON.stringify(proof, null, 2)}\n`,
  );
  console.log(
    `observability proof passed: 7d and 31d leaderboard and harness rows matched the production API`,
  );
  console.log(`evidence: ${evidenceDir}`);
} finally {
  await browser.close();
}

async function waitForSignIn(page) {
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
  try {
    await page.waitForFunction(
      () => {
        const button = Array.from(document.querySelectorAll("button")).find(
          (candidate) => candidate.textContent?.trim() === "Sign in",
        );
        return button instanceof HTMLButtonElement && !button.disabled;
      },
      null,
      { timeout: 60_000 },
    );
  } catch {
    console.log("Sign-in did not hydrate after the first Vite load; reloading once.");
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.getByRole("heading", { name: "Sign in" }).waitFor();
    await page.waitForFunction(
      () => {
        const button = Array.from(document.querySelectorAll("button")).find(
          (candidate) => candidate.textContent?.trim() === "Sign in",
        );
        return button instanceof HTMLButtonElement && !button.disabled;
      },
      null,
      { timeout: 60_000 },
    );
  }
}

async function fetchView(page, requestPath) {
  return page.evaluate(async (pathName) => {
    const response = await fetch(pathName);
    if (!response.ok) throw new Error(`${pathName} returned ${response.status}`);
    const envelope = await response.json();
    return envelope.data;
  }, requestPath);
}

async function assertLeaderboard(page, view) {
  const table = await readTable(page, "leaderboard-table");
  const columns = requireHeaders(table, [
    "Rank",
    "User",
    "Total spend",
    "Most used model",
    "Most used harness",
    "Total requests",
  ]);
  assertEqual(table.rows.length, view.leaders.length, "leaderboard rendered row count");

  for (const [index, leader] of view.leaders.entries()) {
    const cells = table.rows[index];
    assertEqual(cells[columns.Rank], String(leader.rank), `leaderboard rank ${index + 1}`);
    assertEqual(cells[columns.User], leader.user_name, `leaderboard user ${index + 1}`);
    assertEqual(
      cells[columns["Total spend"]],
      currencyFormatter.format(leader.total_spend_usd_10000 / 10_000),
      `leaderboard spend for ${leader.user_name}`,
    );
    assertEqual(
      cells[columns["Most used model"]],
      leader.most_used_model ?? "—",
      `leaderboard model for ${leader.user_name}`,
    );
    assertEqual(
      cells[columns["Most used harness"]],
      leader.most_used_harness?.label ?? "—",
      `leaderboard harness for ${leader.user_name}`,
    );
    assertEqual(
      cells[columns["Total requests"]],
      numberFormatter.format(leader.total_requests),
      `leaderboard requests for ${leader.user_name}`,
    );
  }
  return table;
}

async function assertHarnessUsage(page, view) {
  const table = await readTable(page, "harness-usage-table");
  const columns = requireHeaders(table, [
    "Rank",
    "Harness",
    "Requests",
    "Input tokens",
    "Output tokens",
    "Total tokens",
    "Key",
  ]);
  assertEqual(table.rows.length, view.leaders.length, "harness rendered row count");

  for (const [index, leader] of view.leaders.entries()) {
    const cells = table.rows[index];
    assertEqual(cells[columns.Rank], String(index + 1), `harness rank ${index + 1}`);
    assertEqual(cells[columns.Harness], leader.agent_harness_label, `harness label ${index + 1}`);
    assertEqual(
      cells[columns.Requests],
      numberFormatter.format(leader.total_requests),
      `requests for ${leader.agent_harness_key}`,
    );
    assertEqual(
      cells[columns["Input tokens"]],
      formatTokenCount(leader.prompt_tokens),
      `input tokens for ${leader.agent_harness_key}`,
    );
    assertEqual(
      cells[columns["Output tokens"]],
      formatTokenCount(leader.completion_tokens),
      `output tokens for ${leader.agent_harness_key}`,
    );
    assertEqual(
      cells[columns["Total tokens"]],
      formatTokenCount(leader.total_tokens),
      `total tokens for ${leader.agent_harness_key}`,
    );
    assertEqual(cells[columns.Key], leader.agent_harness_key, `key for harness row ${index + 1}`);
  }
  return table;
}

async function assertHarnessIcons(page, view) {
  const mastraIndex = view.leaders.findIndex((leader) => leader.agent_harness_key === "mastra");
  const ompIndex = view.leaders.findIndex((leader) => leader.agent_harness_key === "oh_my_pi");
  if (mastraIndex < 0 || ompIndex < 0) {
    throw new Error("The 7d demo API must include both Mastra and Oh My Pi harness rows.");
  }

  const rows = page.getByTestId("harness-usage-table").locator("tbody tr");
  const mastraIconCount = await rows
    .nth(mastraIndex)
    .locator('[data-agent-harness-icon="mastra"]')
    .count();
  const ompIconCount = await rows.nth(ompIndex).locator("[data-agent-harness-icon]").count();
  assertEqual(mastraIconCount, 1, "Mastra icon count");
  assertEqual(ompIconCount, 0, "Oh My Pi icon count");
  return { mastraIconCount, ompIconCount };
}

async function selectRange(page, tableTestId, keyHeader, leaders, projectLeader) {
  const rangeToggle = page.getByRole("radio", { name: "Last 31 days" });
  await rangeToggle.click();
  await page.waitForFunction(
    ({ expectedRows, expectedKeyHeader, expectedTableTestId }) => {
      const table = document.querySelector(`[data-testid="${expectedTableTestId}"]`);
      if (!table) return false;
      const headers = Array.from(table.querySelectorAll("thead th")).map((cell) =>
        (cell.textContent ?? "").replace(/\s+/g, " ").trim(),
      );
      const keyIndex = headers.indexOf(expectedKeyHeader);
      const requestIndex =
        headers.indexOf("Total requests") >= 0
          ? headers.indexOf("Total requests")
          : headers.indexOf("Requests");
      if (keyIndex < 0 || requestIndex < 0) return false;
      const rows = Array.from(table.querySelectorAll("tbody tr"));
      if (rows.length !== expectedRows.length) return false;
      return rows.every((row, index) => {
        const cells = Array.from(row.querySelectorAll("td")).map((cell) =>
          (cell.textContent ?? "").replace(/\s+/g, " ").trim(),
        );
        const requests = cells[requestIndex].replace(/,/g, "");
        return (
          cells[keyIndex] === expectedRows[index].key &&
          requests === String(expectedRows[index].requests)
        );
      });
    },
    {
      expectedRows: leaders.map(projectLeader),
      expectedKeyHeader: keyHeader,
      expectedTableTestId: tableTestId,
    },
    { timeout: 60_000 },
  );
  const checked = await rangeToggle.getAttribute("aria-checked");
  assertEqual(checked, "true", `${tableTestId} Last 31 days selection`);
}

async function assertChartSeries(page, expectedCount) {
  await page.waitForFunction(
    (expected) => document.querySelectorAll("g.recharts-area").length === expected,
    expectedCount,
    { timeout: 60_000 },
  );
}

async function readTable(page, testId) {
  return page.getByTestId(testId).evaluate((table) => ({
    headers: Array.from(table.querySelectorAll("thead th")).map((cell) =>
      (cell.textContent ?? "").replace(/\s+/g, " ").trim(),
    ),
    rows: Array.from(table.querySelectorAll("tbody tr")).map((row) =>
      Array.from(row.querySelectorAll("td")).map((cell) =>
        (cell.textContent ?? "").replace(/\s+/g, " ").trim(),
      ),
    ),
  }));
}

function requireHeaders(table, expectedHeaders) {
  const columns = {};
  for (const header of expectedHeaders) {
    const index = table.headers.indexOf(header);
    if (index < 0) throw new Error(`Required table header ${header} is missing.`);
    columns[header] = index;
  }
  return columns;
}

function proofProjection(view, renderedTable) {
  return {
    apiRange: view.range,
    windowStart: view.window_start,
    windowEnd: view.window_end,
    chartSeriesCount: view.chart_users?.length ?? view.chart_harnesses.length,
    seriesPointCount: view.series.length,
    apiLeaders: view.leaders,
    renderedHeaders: renderedTable.headers,
    renderedRows: renderedTable.rows,
  };
}

function formatTokenCount(value) {
  return value == null ? "n/a" : numberFormatter.format(value);
}

function compact(value) {
  return value.replace(/[\s,|·•—]/g, "");
}

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(
      `${label}: expected ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}`,
    );
  }
}

async function capture(page, name) {
  await page.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage: true });
  const snapshot = await page.locator("body").ariaSnapshot();
  await fs.writeFile(path.join(evidenceDir, `${name}.aria.txt`), `${snapshot}\n`);
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}`);
  return value;
}
