import { mkdir } from "node:fs/promises";
import { join } from "node:path";

export interface IsolatedPaths {
  cache: string;
  config: string;
  data: string;
  home: string;
  temp: string;
}

export async function createIsolatedPaths(
  workspace: string,
  harness: string,
): Promise<IsolatedPaths> {
  const root = join(workspace, ".harness", harness);
  const paths = {
    cache: join(root, "cache"),
    config: join(root, "config"),
    data: join(root, "data"),
    home: join(root, "home"),
    temp: join(root, "tmp"),
  };
  await Promise.all(Object.values(paths).map((path) => mkdir(path, { recursive: true })));
  return paths;
}

export function createHarnessEnvironment(
  paths: IsolatedPaths,
  overrides: NodeJS.ProcessEnv,
): NodeJS.ProcessEnv {
  return {
    HOME: paths.home,
    PATH: process.env.PATH ?? "/usr/bin:/bin",
    SHELL: process.env.SHELL ?? "/bin/sh",
    TMPDIR: paths.temp,
    XDG_CACHE_HOME: paths.cache,
    XDG_CONFIG_HOME: paths.config,
    XDG_DATA_HOME: paths.data,
    ...overrides,
  };
}
