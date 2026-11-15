import { randomUUID } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCommand } from "../process.js";
import type { GatewayRuntime, HarnessAdapter, HarnessRun, HarnessRunOptions } from "../types.js";
import { createIsolatedPaths, type IsolatedPaths } from "./isolation.js";
import {
  harnessOpenCodeConfig,
  installGuardrailPlugin,
  openCodeEnvironment,
  runOpenCode,
} from "./opencode.js";

const ROOT = resolve(fileURLToPath(new URL("../../../..", import.meta.url)));
const PLUGIN_SOURCE = join(ROOT, "packages", "opencode-oceans");

/** Packs `@ahstn/opencode-oceans` as npm would publish it, and returns the unpacked folder. */
export async function packOceansPlugin(directory: string): Promise<string> {
  const archive = join(directory, "plugin.tgz");
  await runCommand("bun", ["pm", "pack", "--filename", archive], { cwd: PLUGIN_SOURCE });
  await runCommand("tar", ["-xzf", archive, "-C", directory], { cwd: directory });
  return join(directory, "package");
}

/**
 * The environment for an OpenCode server that loads the packed Oceans plugin. The plugin finds the
 * gateway in `gateways.yml` with `litellm` discovery, as the OpenCode setup page describes.
 * `config` adds OpenCode settings next to the plugin entry.
 */
export async function oceansPluginEnvironment(
  runtime: GatewayRuntime,
  isolated: IsolatedPaths,
  packedPlugin: string,
  config: Record<string, unknown>,
): Promise<NodeJS.ProcessEnv> {
  await writeFile(
    join(isolated.config, "gateways.yml"),
    `providers:\n  oceans:\n    baseUrl: ${JSON.stringify(runtime.baseUrl)}\n    apiKey: OCEANS_API_KEY\n    discovery: {type: litellm}\n`,
    "utf8",
  );
  return openCodeEnvironment(isolated, {
    OCEANS_API_KEY: runtime.apiKey,
    OCEANS_BASE_URL: runtime.baseUrl,
    OPENCODE_CONFIG_CONTENT: JSON.stringify({
      plugins: [{ package: packedPlugin, options: { configDir: isolated.config } }],
      ...config,
    }),
  });
}

/** OpenCode with the packed Oceans plugin, which discovers the gateway's models. */
export class OpenCodePluginAdapter implements HarnessAdapter {
  readonly harness = "opencode" as const;
  readonly label = "OpenCode";
  readonly tools = { read: "read", shell: "shell", write: ["write", "patch"] };
  readonly #runtime: GatewayRuntime;
  #directory: Promise<string> | undefined;
  #packedPlugin: Promise<string> | undefined;

  constructor(runtime: GatewayRuntime) {
    this.#runtime = runtime;
  }

  async run(
    workspace: string,
    prompt: string,
    model: string,
    options: HarnessRunOptions = {},
  ): Promise<HarnessRun> {
    const requestTag = randomUUID();
    this.#directory ??= mkdtemp(join(tmpdir(), "oceans-opencode-plugin-"));
    this.#packedPlugin ??= this.#directory.then(packOceansPlugin);
    const packedPlugin = await this.#packedPlugin;
    const isolated = await createIsolatedPaths(workspace, this.harness);
    await installGuardrailPlugin(isolated, workspace);
    const environment = await oceansPluginEnvironment(this.#runtime, isolated, packedPlugin, {
      ...harnessOpenCodeConfig(this.#runtime, options),
      providers: { oceans: { headers: { "x-oceans-tags": `harness_run=${requestTag}` } } },
    });
    const selected = `oceans/${model}${variant(options)}`;
    return {
      ...(await runOpenCode(workspace, environment, selected, prompt, options)),
      requestTag,
    };
  }

  /** Removes the packed plugin. */
  async close(): Promise<void> {
    const directory = await this.#directory?.catch(() => undefined);
    if (directory) await rm(directory, { recursive: true, force: true });
  }
}

/**
 * The plugin's model variant for the run options, as `#<variant>` after the model. The plugin names
 * effort variants after the effort, and the priority tier variant `fast`. A model has one variant
 * at a time.
 */
function variant(options: HarnessRunOptions): string {
  if (options.effort && options.tier) {
    throw new Error("An OpenCode model variant sets an effort or a tier, not both");
  }
  if (options.tier === "priority") return "#fast";
  return options.effort ? `#${options.effort}` : "";
}
