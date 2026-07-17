import { spawn } from "node:child_process";

export interface CommandOptions {
  cwd: string;
  env?: NodeJS.ProcessEnv;
  timeoutMs?: number;
}

export interface CommandResult {
  stderr: string;
  stdout: string;
}

export async function runCommand(
  command: string,
  args: string[],
  options: CommandOptions,
): Promise<CommandResult> {
  const child = spawn(command, args, {
    cwd: options.cwd,
    env: { ...process.env, ...options.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];
  child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
  child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));

  const { promise, resolve, reject } = Promise.withResolvers<number | null>();
  child.once("error", reject);
  child.once("exit", resolve);

  const timeout = setTimeout(() => {
    child.kill("SIGTERM");
    reject(new Error(`Command timed out after ${options.timeoutMs ?? 180_000}ms: ${command}`));
  }, options.timeoutMs ?? 180_000);

  try {
    const exitCode = await promise;
    const result = {
      stdout: Buffer.concat(stdout).toString("utf8"),
      stderr: Buffer.concat(stderr).toString("utf8"),
    };
    if (exitCode !== 0) {
      throw new Error(
        `Command failed with exit code ${exitCode}: ${command}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
      );
    }
    return result;
  } finally {
    clearTimeout(timeout);
  }
}

export function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}
