#!/usr/bin/env node

import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
const requireFromAdminUi = createRequire(path.join(repoRoot, "crates/admin-ui/web/package.json"));
const { chromium, request } = requireFromAdminUi("playwright");
const { expect: baseExpect } = requireFromAdminUi("playwright/test");
const expect = baseExpect.configure({ timeout: 30_000 });
const baseURL = requiredEnv("OCEANS_VERIFY_BASE_URL");
const evidenceDir = requiredEnv("OCEANS_VERIFY_EVIDENCE_DIR");
const gatewayVersion = requiredEnv("OCEANS_VERIFY_GATEWAY_VERSION");
const adminEmail = requiredEnv("OCEANS_VERIFY_ADMIN_EMAIL");
const adminPassword = requiredEnv("OCEANS_VERIFY_ADMIN_PASSWORD");
const serviceAccountKey = requiredEnv("OCEANS_VERIFY_SKILLS_SERVICE_ACCOUNT_KEY");
const suffix = randomUUID().replaceAll("-", "").slice(0, 12);
const fixturePath = (version) =>
  path.join(repoRoot, `crates/admin-ui/web/e2e/fixtures/skills/review-code-v${version}.zip`);
const actions = [];
const createdUsers = [];
const proof = {
  feature: "skills",
  runId: process.env.OCEANS_VERIFY_RUN_ID,
  entryUrl: `${baseURL}/admin/skills`,
  gatewayVersion,
  storage: {
    endpoint: requiredEnv("OCEANS_VERIFY_SKILLS_ENDPOINT"),
    bucket: requiredEnv("OCEANS_VERIFY_SKILLS_BUCKET"),
    prefix: requiredEnv("OCEANS_VERIFY_SKILLS_PREFIX"),
  },
  actions,
  cleanup: {
    usersDeactivated: [],
    skills: "Stored in the run-local database; control-oceans-admin cleanup removes that database.",
    objects: "Run prefix is removed by control-oceans-admin cleanup.",
  },
  passed: false,
};

await fs.mkdir(evidenceDir, { recursive: true });
const browser = await chromium.launch({ headless: true });
const adminContext = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const adminPage = await adminContext.newPage();
let currentPage = adminPage;
let failure;

try {
  await adminPage.goto(proof.entryUrl, { waitUntil: "domcontentloaded" });
  await expect(adminPage.getByRole("heading", { name: "Sign in", exact: true })).toBeVisible();
  await capture(adminPage, "01-skills-login");
  await fillSignIn(adminPage, adminEmail, adminPassword);
  await expect(adminPage.getByRole("heading", { name: "Skills", exact: true })).toBeVisible();
  actions.push({
    action: "open protected Skills page and sign in as platform admin",
    result: "Skills catalog visible",
  });

  const owner = await createUser(adminContext.request, "owner");
  const peer = await createUser(adminContext.request, "reader");
  const ownerContext = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const ownerPage = await ownerContext.newPage();
  currentPage = ownerPage;
  await signIn(ownerPage, owner);
  await ownerPage.getByRole("link", { name: "Skills", exact: true }).first().click();
  await expect(ownerPage.getByRole("heading", { name: "Skills", exact: true })).toBeVisible();
  proof.catalog = { before: await filterCatalog(ownerPage, owner.namespace) };
  assert.equal(proof.catalog.before.apiCount, 0, "New namespace starts with an empty catalog");
  await capture(ownerPage, "02-skills-before");
  actions.push({
    action: "regular user follows Skills sidebar and filters own namespace",
    result: "Empty catalog before upload",
  });

  const initial = await uploadFirstThroughUi(ownerPage, owner);
  const saved = await verifyOwnerVersions(ownerPage, initial);
  proof.owner = { userId: owner.id, namespace: owner.namespace, skillId: saved.skill.id };
  proof.versions = saved.versions.map(({ version, sha256 }) => ({ version, sha256 }));
  proof.defaultVersion = saved.skill.default_version;

  const peerContext = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const peerPage = await peerContext.newPage();
  currentPage = peerPage;
  await signIn(peerPage, peer);
  const duplicateNamespace = await peerPage.request.post(`${baseURL}/api/v1/skills/namespace`, {
    data: { handle: owner.namespace },
  });
  assert.equal(
    duplicateNamespace.status(),
    409,
    "A namespace already owned by another user cannot be claimed",
  );
  assert.equal(
    await json(peerPage.request, "/api/v1/skills/namespace"),
    null,
    "Failed namespace claim leaves the user without a namespace",
  );
  const duplicate = await uploadFirstThroughUi(peerPage, peer);
  assert.notEqual(duplicate.skill.id, initial.skill.id);
  assert.equal(duplicate.skill.name, initial.skill.name);
  assert.equal(duplicate.skill.owner_user_id, peer.id);
  proof.peer = { userId: peer.id, namespace: peer.namespace, skillId: duplicate.skill.id };
  actions.push({
    action: "second user uploads the same skill name",
    result: "Separate owner namespace and skill ID",
  });

  await verifySharedAccess(peerPage, adminContext.request, saved, owner);
  proof.authentication = await verifyAuthentication(saved.skill.id);
  await verifyNamespaceRules(ownerContext.request, peerContext.request, owner, peer);
  currentPage = ownerPage;
  await ownerPage.goto(`${baseURL}/admin/skills`, {
    waitUntil: "domcontentloaded",
  });
  proof.catalog.after = await filterCatalog(ownerPage, owner.namespace);
  assert.deepEqual(proof.catalog.after.skillIds, [saved.skill.id]);
  await expect(
    ownerPage.getByRole("link", { name: `${owner.namespace}/review-code`, exact: true }),
  ).toBeVisible();
  await capture(ownerPage, "07-skills-catalog-after");
  proof.passed = true;
} catch (error) {
  failure = error;
  // A failed sign-in can still contain a password. Capture only the authenticated Skills surface.
  if (/\/admin\/skills(?:[/?]|$)/.test(currentPage.url())) {
    await capture(currentPage, "99-skills-failure").catch(() => {});
  }
} finally {
  for (const user of createdUsers) {
    try {
      const response = await adminContext.request.post(
        `${baseURL}/api/v1/admin/identity/users/${user.id}/deactivate`,
      );
      assert.equal(response.status(), 200, "Deactivate temporary verification user");
      proof.cleanup.usersDeactivated.push(user.id);
    } catch (error) {
      failure ??= error;
    }
  }
  proof.passed = proof.passed && !failure;
  proof.generatedAt = new Date().toISOString();
  await fs.writeFile(
    path.join(evidenceDir, "skills-proof.json"),
    `${JSON.stringify(proof, null, 2)}\n`,
  );
  await browser.close();
}
if (failure) throw sanitizedFailure(failure);
console.log(
  "skills proof passed: browser upload, immutable versions, shared reads, owner-only changes, and archive digests matched the production API",
);
console.log(`evidence: ${evidenceDir}`);

async function createUser(adminApi, label) {
  const user = {
    name: `Skills Verification ${label}`,
    email: `skills-${label}-${suffix}@example.com`,
    password: `Skills-${randomUUID()}-9`,
    namespace: `verify-${label}-${suffix}`,
  };
  const created = await json(adminApi, "/api/v1/admin/identity/users", {
    method: "POST",
    data: {
      name: user.name,
      email: user.email,
      auth_mode: "password",
      global_role: "user",
      tags: [],
    },
  });
  assert.equal(created.data.kind, "password_invite");
  user.id = created.data.user.id;
  createdUsers.push(user);
  const invite = new URL(created.data.invite_url, baseURL).pathname.split("/").findLast(Boolean);
  assert(invite, "Production invitation response must include a token");
  const anonymous = await request.newContext();
  try {
    const response = await anonymous.post(`${baseURL}/api/v1/auth/invitations/${invite}/password`, {
      data: { password: user.password },
    });
    assert.equal(response.status(), 200, "Complete temporary user password invitation");
  } finally {
    await anonymous.dispose();
  }
  return user;
}

async function signIn(page, user) {
  await page.goto(`${baseURL}/admin/login?redirect=/skills`, { waitUntil: "domcontentloaded" });
  await fillSignIn(page, user.email, user.password);
  await expect(page.getByRole("heading", { name: "Skills", exact: true })).toBeVisible();
}

async function fillSignIn(page, email, password) {
  const signIn = page.getByRole("button", { name: "Sign in", exact: true });
  await expect(signIn).toBeEnabled({ timeout: 60_000 });
  await page.getByLabel("Email", { exact: true }).fill(email);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await signIn.click();
}

async function filterCatalog(page, namespace) {
  const input = page.getByLabel("Owner namespace", { exact: true });
  // SSR can expose this controlled input before hydration attaches its change handler.
  // Retrying this read-only filter proves that the live form preserved and submitted its value.
  await expect(async () => {
    await input.fill(namespace);
    await expect(input).toHaveValue(namespace, { timeout: 1_000 });
    await page.getByRole("button", { name: "Filter", exact: true }).click();
    await expect(page).toHaveURL((url) => url.searchParams.get("namespace") === namespace, {
      timeout: 5_000,
    });
    await expect(input).toHaveValue(namespace, { timeout: 1_000 });
  }).toPass({ timeout: 20_000, intervals: [250, 500, 1_000] });

  const items = await json(
    page.request,
    `/api/v1/skills?namespace=${encodeURIComponent(namespace)}`,
  );
  let uiCount = 0;
  if (items.length === 0) {
    await expect(page.getByText("No skills found", { exact: true })).toBeVisible();
    await expect(page.getByRole("table")).toHaveCount(0);
  } else {
    for (const skill of items) {
      await expect(
        page.getByRole("link", { name: `${skill.namespace}/${skill.name}`, exact: true }),
      ).toBeVisible();
    }
    const rows = page.getByRole("table").getByRole("row");
    await expect(rows).toHaveCount(items.length + 1);
    uiCount = (await rows.count()) - 1;
    assert.equal(uiCount, items.length, "Filtered UI catalog matches the production API");
  }
  return {
    url: page.url(),
    uiCount,
    apiCount: items.length,
    skillIds: items.map((skill) => skill.id),
  };
}

async function uploadFirstThroughUi(page, owner) {
  await page.getByRole("button", { name: "Upload skill", exact: true }).click();
  const namespace = page.getByRole("dialog", { name: "Choose your skill namespace", exact: true });
  await namespace.getByLabel("Namespace", { exact: true }).fill(owner.namespace);
  await namespace.getByRole("button", { name: "Claim namespace", exact: true }).click();
  await uploadDialog(page, "Upload a skill", 1);
  await expect(
    page.getByRole("heading", { name: `${owner.namespace}/review-code`, exact: true }),
  ).toBeVisible();
  await expect(page.getByTestId("skill-instructions")).toContainText("checklist version 1");
  const detail = await json(page.request, `/api/v1/skills/by-name/${owner.namespace}/review-code`);
  assert.equal(detail.skill.owner_user_id, owner.id);
  assert.equal(detail.skill.default_version, 1);
  assert.equal(detail.skill.latest_version, 1);
  assert.equal(detail.versions.length, 1);
  assert.match(detail.versions[0].sha256, /^[a-f0-9]{64}$/);
  actions.push({
    action: "claim immutable namespace and upload ZIP through UI",
    result: `${owner.namespace}/review-code version 1`,
  });
  return detail;
}

async function uploadDialog(page, name, version) {
  const dialog = page.getByRole("dialog", { name, exact: true });
  await dialog.getByLabel("ZIP archive").setInputFiles(fixturePath(version));
  await dialog.getByRole("button", { name: "Upload skill", exact: true }).click();
}

async function verifyOwnerVersions(page, initial) {
  const skillPath = `/api/v1/skills/${initial.skill.id}`;
  await page
    .getByRole("navigation", { name: "Skill files" })
    .getByRole("button", { name: "references/checklist.md", exact: true })
    .click();
  await expect(page.getByTestId("skill-file-text")).toContainText("Checklist version 1");
  const file = await json(
    page.request,
    `${skillPath}/versions/1/files?path=references%2Fchecklist.md`,
  );
  const rendered = await page.getByTestId("skill-file-text").textContent();
  assert.equal(rendered, file.content, "Rendered file preview matches the production API");
  await capture(page, "03-skills-file-preview");

  await page.getByRole("button", { name: "Upload new version", exact: true }).click();
  await uploadDialog(page, "Upload a new version", 2);
  await expect(page.getByTestId("skill-instructions")).toContainText("checklist version 2");
  const appended = await json(page.request, skillPath);
  assert.equal(appended.skill.latest_version, 2);
  assert.equal(appended.skill.default_version, 1, "Appending a version must preserve the default");
  assert.equal(appended.versions.length, 2);
  await capture(page, "04-skills-new-version");

  await page.getByRole("button", { name: "Set as default", exact: true }).click();
  await expect(page.getByText("Default version updated", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Set as default", exact: true })).toBeDisabled();
  const saved = await json(page.request, skillPath);
  assert.equal(saved.skill.default_version, 2);
  const downloadEvent = page.waitForEvent("download");
  await page.getByRole("link", { name: "Download ZIP", exact: true }).click();
  const download = await downloadEvent;
  assert.equal(await download.failure(), null, "Browser ZIP download completes");
  const digest = createHash("sha256")
    .update(await fs.readFile(await download.path()))
    .digest("hex");
  assert.equal(digest, saved.versions.find((item) => item.version === 2).sha256);
  await capture(page, "05-skills-owner-default");

  const old = await json(page.request, `${skillPath}/versions/1`);
  assert(old.instructions.includes("checklist version 1"), "Old instructions remain unchanged");
  const oldArchive = await page.request.get(`${baseURL}${skillPath}/versions/1/archive`);
  assert.equal(oldArchive.status(), 200);
  assert.equal(
    createHash("sha256")
      .update(await oldArchive.body())
      .digest("hex"),
    initial.versions[0].sha256,
  );
  actions.push({
    action: "upload version 2, change default, and download from the browser",
    result:
      "API saved default 2; downloaded digest matches; version 1 content and digest remain unchanged",
  });
  return saved;
}

async function verifySharedAccess(peerPage, adminApi, saved, owner) {
  const skillPath = `/api/v1/skills/${saved.skill.id}`;
  await peerPage.goto(`${baseURL}/admin/skills/${saved.skill.id}`, {
    waitUntil: "domcontentloaded",
  });
  await expect(
    peerPage.getByRole("heading", { name: `${owner.namespace}/review-code`, exact: true }),
  ).toBeVisible();
  await expect(peerPage.getByTestId("skill-instructions")).toContainText("checklist version 2");
  await expect(
    peerPage.getByRole("button", { name: "Upload new version", exact: true }),
  ).toHaveCount(0);
  await expect(peerPage.getByRole("button", { name: "Set as default", exact: true })).toHaveCount(
    0,
  );
  await capture(peerPage, "06-skills-other-owner");
  for (const api of [peerPage.request, adminApi]) {
    const read = await json(api, skillPath);
    assert.equal(read.skill.default_version, 2);
    const append = await api.post(`${baseURL}${skillPath}/versions`, {
      headers: { "content-type": "application/zip" },
      data: await fs.readFile(fixturePath(2)),
    });
    assert.equal(
      append.status(),
      403,
      "Only owner may append, including when caller is platform admin",
    );
    const choose = await api.put(`${baseURL}${skillPath}/default-version`, {
      data: { version: 1 },
    });
    assert.equal(choose.status(), 403, "Only owner may change default");
  }
  actions.push({
    action: "other user views skill; other user and platform admin attempt updates",
    result: "Shared read succeeds, owner controls hidden, both write paths return 403",
  });
}

async function verifyAuthentication(skillId) {
  const anonymous = await request.newContext();
  const checks = {
    anonymous: "passed",
    serviceAccount: "pending",
  };
  try {
    for (const endpoint of ["/api/v1/skills", `/api/v1/skills/${skillId}/versions/1/archive`]) {
      assert.equal(
        (await anonymous.get(`${baseURL}${endpoint}`)).status(),
        401,
        "Unauthenticated Skills request",
      );
    }
  } finally {
    await anonymous.dispose();
  }
  const serviceAccount = await request.newContext({
    extraHTTPHeaders: { authorization: `Bearer ${serviceAccountKey}` },
  });
  try {
    assert.equal(
      (await serviceAccount.get(`${baseURL}/api/v1/skills/${skillId}`)).status(),
      200,
      "Service account can read Skills",
    );
    const body = {
      headers: { "content-type": "application/zip" },
      data: await fs.readFile(fixturePath(2)),
    };
    assert.equal(
      (await serviceAccount.post(`${baseURL}/api/v1/skills`, body)).status(),
      403,
      "Service account cannot create a skill",
    );
    assert.equal(
      (await serviceAccount.post(`${baseURL}/api/v1/skills/${skillId}/versions`, body)).status(),
      403,
      "Service account cannot append a version",
    );
    assert.equal(
      (
        await serviceAccount.post(`${baseURL}/api/v1/skills/namespace`, {
          data: { handle: `verify-bot-${suffix}` },
        })
      ).status(),
      403,
      "Service account cannot claim a namespace",
    );
    assert.equal(
      (
        await serviceAccount.put(`${baseURL}/api/v1/skills/${skillId}/default-version`, {
          data: { version: 1 },
        })
      ).status(),
      403,
      "Service account cannot select default",
    );
    checks.serviceAccount = "passed";
  } finally {
    await serviceAccount.dispose();
  }
  actions.push({
    action: "check anonymous and service-account access",
    result: "Anonymous reads return 401; service-account reads succeed and all writes return 403",
  });
  return checks;
}

async function verifyNamespaceRules(ownerApi, peerApi, owner, peer) {
  const rename = await ownerApi.post(`${baseURL}/api/v1/skills/namespace`, {
    data: { handle: `renamed-${suffix}` },
  });
  assert.equal(rename.status(), 409, "Namespace cannot be changed after it is claimed");
  assert.equal((await json(ownerApi, "/api/v1/skills/namespace")).handle, owner.namespace);
  assert.equal((await json(peerApi, "/api/v1/skills/namespace")).handle, peer.namespace);
  actions.push({
    action: "attempt namespace change and namespace reuse",
    result: "Both return 409; original namespaces remain unchanged",
  });
}

async function json(api, route, options = {}) {
  const response = await api.fetch(`${baseURL}${route}`, options);
  assert.equal(response.status(), 200, `Production API ${route.split("?")[0]}`);
  return response.json();
}

async function capture(page, name) {
  await page.screenshot({ path: path.join(evidenceDir, `${name}.png`), fullPage: true });
  await fs.writeFile(
    path.join(evidenceDir, `${name}.aria.txt`),
    `${await page.locator("body").ariaSnapshot()}\n`,
  );
}

function requiredEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable ${name}`);
  return value;
}

function sanitizedFailure(error) {
  // Playwright errors carry request headers in Call log and matcher metadata. Keep neither.
  let message = String(error?.message ?? "Skills verification failed")
    .replace(
      /^.*(?:authorization|proxy-authorization|cookie|set-cookie)\s*:.*$/gim,
      "[redacted header]",
    )
    .split(/\r?\n/)[0];
  for (const secret of [
    serviceAccountKey,
    adminPassword,
    ...createdUsers.map((user) => user.password),
  ]) {
    if (secret) message = message.replaceAll(secret, "[redacted]");
  }
  message = message.replace(/\/auth\/invitations\/[^/\s]+/g, "/auth/invitations/[redacted]");
  return new Error(`Skills verification failed: ${message.slice(0, 400)}`);
}
