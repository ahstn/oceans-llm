import { randomUUID } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { crc32, deflateSync } from "node:zlib";
import { OpenCode, type OpenCodeClient } from "@opencode/client";
import { afterAll, beforeAll, expect, inject, test } from "vitest";
import { z } from "zod";

import { createHarnessEnvironment, createIsolatedPaths } from "./adapters/isolation.js";
import { withPrivateServer } from "./adapters/opencode.js";
import { assertSuccessful, fetchWithTimeout, GatewayAdminClient } from "./gateway-client.js";
import { runCommand } from "./process.js";

const runtime = inject("gateway");
const ROOT = resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const PLUGIN = join(ROOT, "packages/opencode-oceans");
const PNG = redPng().toString("base64");
const ListingSchema = z.object({
  data: z.array(
    z.object({
      id: z.string(),
      context_length: z.number().positive(),
      architecture: z.object({ input_modalities: z.array(z.string()) }),
    }),
  ),
});
const RichSchema = z.object({
  data: z.array(
    z.object({
      model_group: z.string(),
      max_input_tokens: z.number().positive(),
      max_output_tokens: z.number().positive(),
      input_cost_per_token: z.number().nonnegative(),
      output_cost_per_token: z.number().nonnegative(),
    }),
  ),
});
const LoggedRequestSchema = z.object({ path: z.string(), body: z.record(z.string(), z.unknown()) });

let root: string;
let packedPlugin: string;

beforeAll(async () => {
  root = await mkdtemp(join(tmpdir(), "oceans-opencode-plugin-"));
  const archive = join(root, "plugin.tgz");
  await runCommand("bun", ["pm", "pack", "--filename", archive], { cwd: PLUGIN });
  await runCommand("tar", ["-xzf", archive, "-C", root], { cwd: root });
  packedPlugin = join(root, "package");
});
afterAll(async () => {
  if (root) await rm(root, { recursive: true, force: true });
});

test("packed Oceans plugin discovers authorized models, native protocols, limits, prices, and capability restrictions", async () => {
  const [listing, rich] = await Promise.all([
    discovery("/v1/models", ListingSchema),
    discovery("/model_group/info", RichSchema),
  ]);
  await withPluginHost(runtime.pluginModels.openai, randomUUID(), async (client, workspace) => {
    const models = (await client.model.list({ location: { directory: workspace } })).data.filter(
      (model) => model.providerID === "oceans",
    );
    expect(models.map((model) => model.id)).toContain(runtime.pluginModels.openai);
    expect(models.map((model) => model.id)).toContain(runtime.pluginModels.anthropic);
    expect(models.map((model) => model.id)).not.toContain(runtime.pluginModels.hidden);
    expect(listing.data.map((model) => model.id)).not.toContain(runtime.pluginModels.hidden);
    for (const id of [runtime.pluginModels.openai, runtime.pluginModels.anthropic]) {
      const model = models.find((candidate) => candidate.id === id);
      const source = listing.data.find((candidate) => candidate.id === id);
      const rates = rich.data.find((candidate) => candidate.model_group === id);
      expect(model, `OpenCode did not discover ${id}`).toBeDefined();
      expect(source, `Gateway did not list ${id}`).toBeDefined();
      expect(rates, `Gateway did not return rich metadata for ${id}`).toBeDefined();
      expect(model!.capabilities.input).toEqual(expect.arrayContaining(["text", "image", "pdf"]));
      expect(model!.capabilities.tools).toBe(true);
      expect(model!.capabilities.output).toEqual(["text"]);
      expect(model!.limit).toEqual({
        context: source!.context_length,
        input: rates!.max_input_tokens,
        output: rates!.max_output_tokens,
      });
      expect(model!.cost[0]?.input).toBeCloseTo(rates!.input_cost_per_token * 1_000_000, 8);
      expect(model!.cost[0]?.output).toBeCloseTo(rates!.output_cost_per_token * 1_000_000, 8);
      expect(model!.variants.map((variant) => variant.id)).toEqual(
        expect.arrayContaining(["low", "high", "fast"]),
      );
      expect(model!.package).toBe(
        id === runtime.pluginModels.openai
          ? "@opencode/ai/providers/openai-compatible/responses"
          : "@opencode/ai/providers/anthropic-compatible",
      );
    }
    const anthropic = models.find((model) => model.id === runtime.pluginModels.anthropic)!;
    expect(anthropic.variants.map((variant) => variant.id)).not.toEqual(
      expect.arrayContaining(["xhigh"]),
    );
    expect(anthropic.variants.map((variant) => variant.id)).not.toContain("max");
    const restricted = models.find((model) => model.id === runtime.pluginModels.textOnly);
    expect(restricted?.capabilities).toMatchObject({
      tools: false,
      input: ["text"],
      output: ["text"],
    });
    // The effort ceiling stops at `medium`; service-tier variants are independent of it.
    expect(restricted?.variants.map((variant) => variant.id)).toEqual([
      "low",
      "medium",
      "flex",
      "fast",
      "ultrafast",
    ]);
  });
});

const cases = [
  { family: "openai", model: runtime.pluginModels.openai, variant: "low" },
  { family: "openai", model: runtime.pluginModels.openai, variant: "high" },
  { family: "openai", model: runtime.pluginModels.openai, variant: "fast" },
  { family: "anthropic", model: runtime.pluginModels.anthropic, variant: "low" },
  { family: "anthropic", model: runtime.pluginModels.anthropic, variant: "high" },
  { family: "anthropic", model: runtime.pluginModels.anthropic, variant: "fast" },
];
test.each(cases)(
  "packed Oceans plugin: $family $variant sends native variant settings and attachments and records live usage",
  async ({ family, model, variant }) => {
    const fast = variant === "fast";
    const requestTag = randomUUID();
    const admin = new GatewayAdminClient({ ...runtime, model });
    await admin.login();
    await withPluginHost(model, requestTag, async (client, workspace, wirePath) => {
      const session = await client.session.create({
        title: `Oceans plugin ${family} ${variant}`,
        location: { directory: workspace },
        model: { providerID: "oceans", id: model, variant },
      });
      try {
        await client.session.prompt({
          sessionID: session.id,
          text: "Return the image's dominant color, then the word written in the PDF. Use uppercase, separated by one space, with no other text. Do not call tools.",
          files: [
            { name: "color.png", uri: `data:image/png;base64,${PNG}` },
            {
              name: "word.pdf",
              uri: `data:application/pdf;base64,${textPdf().toString("base64")}`,
            },
          ],
        });
        await client.session.wait(
          { sessionID: session.id },
          { signal: AbortSignal.timeout(120_000) },
        );
        const messages = (await client.message.list({ sessionID: session.id })).data;
        const assistant = messages.find((message) => message.type === "assistant");
        if (!assistant || assistant.type !== "assistant")
          throw new Error("OpenCode produced no assistant response");
        if (assistant.error)
          throw new Error(
            `${assistant.error.type} (${assistant.error.status ?? "no status"}): ${assistant.error.message}`,
          );
        expect(
          assistant.content
            .filter((part) => part.type === "text")
            .map((part) => part.text)
            .join("")
            .trim(),
        ).toBe("RED ORBIT");
        expect(assistant.content.some((part) => part.type === "tool")).toBe(false);
        expect(assistant.tokens?.input).toBeGreaterThan(0);
        expect(
          (assistant.tokens?.output ?? 0) + (assistant.tokens?.reasoning ?? 0),
        ).toBeGreaterThan(0);
        expect(assistant.cost).toBeGreaterThanOrEqual(0);
        const log = await admin.waitForSuccessfulModelLog(requestTag);
        const detail = await admin.getRequestLogDetail(log.request_log_id);
        const request = LoggedRequestSchema.parse(JSON.parse(await readFile(wirePath, "utf8")));
        // Messages request logs contain the normalized Chat representation, not native wire JSON.
        expect(
          z.object({ body: z.object({ model: z.string() }) }).parse(detail.payload?.request_json)
            .body.model,
        ).toBe(model);
        expect(log.provider_key).toBe("openrouter");
        expect(log.model_key).toBe(model);
        expect(
          detail.attempts.find((attempt) => attempt.status_code === 200)?.upstream_model,
        ).toMatch(family === "openai" ? /^openai\// : /^anthropic\//);
        expect(request.body.model).toBe(model);
        const wire = JSON.stringify(request.body);
        expect(wire).toContain(family === "anthropic" ? '"type":"image"' : '"type":"input_image"');
        expect(wire).toContain(
          family === "anthropic" ? '"type":"document"' : '"type":"input_file"',
        );
        if (family === "anthropic") {
          expect(request.path).toBe("/v1/messages");
          expect(request.body.thinking).toMatchObject({ type: "adaptive" });
          if (fast) expect(request.body.speed).toBe("fast");
          else expect(request.body.output_config).toMatchObject({ effort: variant });
          expect(request.body.max_tokens).toBe(2048);
        } else {
          expect(request.path).toBe("/v1/responses");
          if (fast) expect(request.body.service_tier).toBe("priority");
          else expect(request.body.reasoning).toMatchObject({ effort: variant });
          expect(request.body.max_output_tokens).toBe(2048);
        }
        expect(log.service_tier).toBe(fast ? "priority" : null);
        const tokens = assistant.tokens!;
        expect(tokens.input + tokens.cache.read + tokens.cache.write).toBe(log.prompt_tokens);
        expect(tokens.output + tokens.reasoning).toBe(log.completion_tokens);
        expect(log.cost_usd_10000).not.toBeNull();
        // OpenCode prices every variant at the standard rate; Oceans bills the served tier.
        if (log.served_service_tier === "priority")
          expect(log.cost_usd_10000! / 10_000).toBeGreaterThan(assistant.cost!);
        else
          expect(Math.abs(assistant.cost! - log.cost_usd_10000! / 10_000)).toBeLessThanOrEqual(
            0.0001,
          );
        console.info(
          `Live plugin pass: ${family}/${variant}; ${request.path}; request ${log.request_log_id}; tier ${log.service_tier ?? "none"} -> ${log.served_service_tier ?? "unreported"}; tokens ${log.prompt_tokens}/${log.completion_tokens}; session $${assistant.cost}; oceans $${log.cost_usd_10000! / 10_000}`,
        );
      } finally {
        await client.session.remove({ sessionID: session.id });
      }
    });
  },
);

async function discovery<T>(path: string, schema: z.ZodType<T>): Promise<T> {
  const response = await fetchWithTimeout(`${runtime.baseUrl}${path}`, {
    headers: { authorization: `Bearer ${runtime.apiKey}` },
  });
  await assertSuccessful(response, "plugin metadata discovery");
  return schema.parse(await response.json());
}

async function withPluginHost<T>(
  model: string,
  requestTag: string,
  use: (client: OpenCodeClient, workspace: string, wirePath: string) => Promise<T>,
): Promise<T> {
  const workspace = await mkdtemp(join(root, "workspace-"));
  try {
    const isolated = await createIsolatedPaths(workspace, "opencode-plugin");
    const wirePath = join(isolated.config, "wire-request.json");
    const pluginDirectory = join(isolated.config, "plugins");
    await mkdir(pluginDirectory, { recursive: true });
    await writeFile(join(pluginDirectory, "wire-probe.ts"), wireProbeSource(wirePath), "utf8");
    await writeFile(
      join(isolated.config, "gateways.yml"),
      `providers:\n  oceans:\n    baseUrl: ${JSON.stringify(runtime.baseUrl)}\n    apiKey: OCEANS_API_KEY\n    discovery: {type: litellm}\n`,
      "utf8",
    );
    const isOpenAI = model === runtime.pluginModels.openai;
    const environment = createHarnessEnvironment(isolated, {
      OCEANS_API_KEY: runtime.apiKey,
      XDG_STATE_HOME: join(isolated.home, "state"),
      OPENCODE_CONFIG_DIR: isolated.config,
      OPENCODE_DISABLE_AUTOUPDATE: "true",
      OPENCODE_DISABLE_CLAUDE_CODE: "true",
      OPENCODE_DISABLE_FILEWATCHER: "true",
      OPENCODE_DISABLE_FREE_MODELS: "true",
      OPENCODE_DISABLE_MODELS_FETCH: "true",
      OPENCODE_CONFIG_CONTENT: JSON.stringify({
        plugins: [{ package: packedPlugin, options: { configDir: isolated.config } }],
        providers: {
          oceans: {
            headers: { "x-oceans-tags": `harness_run=${requestTag}` },
            body: isOpenAI ? { max_output_tokens: 2048, store: false } : { max_tokens: 2048 },
          },
        },
        agents: {
          build: {
            // A one-step limit injects an assistant prefill, which current Claude models reject.
            steps: 2,
            system: "Answer the user using the provided attachments. Do not call any tools.",
          },
        },
        permissions: [{ action: "*", effect: "deny", resource: "*" }],
      }),
    });
    return await withPrivateServer(workspace, environment, async (server) => {
      const password = server.env.OPENCODE_PASSWORD;
      if (!password) throw new Error("Private OpenCode server has no password");
      const client = OpenCode.make({
        baseUrl: server.url,
        headers: {
          authorization: `Basic ${Buffer.from(`opencode:${password}`).toString("base64")}`,
        },
      });
      return use(client, workspace, wirePath);
    });
  } finally {
    await rm(workspace, { recursive: true, force: true });
  }
}

/** Observes real native requests without changing them or persisting authorization headers. */
function wireProbeSource(file: string): string {
  return `
import { Plugin } from "@opencode/plugin/effect";
import { Effect } from "effect";
export default Plugin.define({
  id: "oceans.harness.wire-probe",
  effect: (ctx) => ctx.session.hook("http.request", (event) => Effect.gen(function* () {
    if (event.kind !== "primary") return;
    const body = yield* Effect.promise(() => event.request.clone().json());
    const path = new URL(event.request.url).pathname;
    yield* Effect.promise(() => Bun.write(${JSON.stringify(file)}, JSON.stringify({path, body})));
  }), {providerID: "oceans"}),
});
`;
}

function redPng(): Buffer {
  const chunk = (type: string, data: Buffer): Buffer => {
    const name = Buffer.from(type);
    const header = Buffer.alloc(4);
    header.writeUInt32BE(data.length);
    const checksum = Buffer.alloc(4);
    checksum.writeUInt32BE(crc32(Buffer.concat([name, data])));
    return Buffer.concat([header, name, data, checksum]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(64, 0);
  header.writeUInt32BE(64, 4);
  header[8] = 8;
  header[9] = 2;
  const pixels = Buffer.alloc(64 * (1 + 64 * 3));
  for (let row = 0; row < 64; row++) {
    for (let column = 0; column < 64; column++) pixels[row * 193 + 1 + column * 3] = 255;
  }
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(pixels)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** A valid, single-page PDF; the answer is only in its bytes, never in the prompt. */
function textPdf(): Buffer {
  const stream = "BT /F1 24 Tf 20 50 Td (ORBIT) Tj ET\n";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}endstream`,
  ];
  let document = "%PDF-1.4\n";
  const offsets: number[] = [];
  for (const [index, object] of objects.entries()) {
    offsets.push(Buffer.byteLength(document));
    document += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }
  const xref = Buffer.byteLength(document);
  document += `xref\n0 6\n0000000000 65535 f \n${offsets.map((offset) => `${String(offset).padStart(10, "0")} 00000 n \n`).join("")}trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(document);
}
