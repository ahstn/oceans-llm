import { spawn, type ChildProcess } from "node:child_process";

export interface CommandOptions {
  cwd: string;
  env?: NodeJS.ProcessEnv;
  stdin?: string;
  timeoutMs?: number;
}

export interface CommandResult {
  stderr: string;
  stdout: string;
}

interface ProcessCompletion {
  error?: Error;
  exitCode: number | null;
  signal: NodeJS.Signals | null;
}

const DEFAULT_TIMEOUT_MS = 180_000;
const TERMINATION_GRACE_MS = 5_000;

export async function runCommand(
  command: string,
  args: string[],
  options: CommandOptions,
): Promise<CommandResult> {
  const child = spawn(command, args, {
    cwd: options.cwd,
    env: options.env,
    stdio: ["pipe", "pipe", "pipe"],
  });
  child.stdin.end(options.stdin);
  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];
  child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
  child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));

  const completion = new Promise<ProcessCompletion>((resolve) => {
    child.once("error", (error) => resolve({ error, exitCode: null, signal: null }));
    child.once("close", (exitCode, signal) => resolve({ exitCode, signal }));
  });
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const initialCompletion = await waitForCompletion(completion, timeoutMs);
  if (!initialCompletion) {
    const stopped = await stopTimedOutCommand(child, completion);
    const result = commandOutput(stdout, stderr);
    const termination = stopped ? "" : "\nProcess did not close after SIGKILL.";
    throw new Error(
      `Command timed out after ${timeoutMs}ms: ${command}${termination}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }

  if (initialCompletion.error) {
    throw new Error(`Command failed to start: ${command}`, { cause: initialCompletion.error });
  }
  const result = commandOutput(stdout, stderr);
  if (initialCompletion.exitCode !== 0) {
    const status = initialCompletion.signal
      ? `signal ${initialCompletion.signal}`
      : `exit code ${initialCompletion.exitCode}`;
    throw new Error(
      `Command failed with ${status}: ${command}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }
  return result;
}

async function stopTimedOutCommand(
  child: ChildProcess,
  completion: Promise<ProcessCompletion>,
): Promise<ProcessCompletion | undefined> {
  child.kill("SIGTERM");
  const terminated = await waitForCompletion(completion, TERMINATION_GRACE_MS);
  if (terminated) return terminated;

  child.kill("SIGKILL");
  return waitForCompletion(completion, TERMINATION_GRACE_MS);
}

function commandOutput(stdout: Buffer[], stderr: Buffer[]): CommandResult {
  return {
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
  };
}

async function waitForCompletion(
  completion: Promise<ProcessCompletion>,
  timeoutMs: number,
): Promise<ProcessCompletion | undefined> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<undefined>((resolve) => {
    timer = setTimeout(() => resolve(undefined), timeoutMs);
  });
  try {
    return await Promise.race([completion, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

export function delay(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}
