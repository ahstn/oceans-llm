import { mkdir } from "node:fs/promises";
import { join } from "node:path";

export interface IsolatedPaths {
  cache: string;
  config: string;
  data: string;
  home: string;
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
  };
  await Promise.all(Object.values(paths).map((path) => mkdir(path, { recursive: true })));
  return paths;
}
