import { describe, expect, test } from "vitest";

import { runCommand } from "./process.js";

describe("runCommand", () => {
  test("passes option-shaped input through stdin without argument parsing", async () => {
    const input = "--model must-remain-prompt-text";
    const result = await runCommand(
      process.execPath,
      ["-e", "process.stdin.pipe(process.stdout)"],
      { cwd: process.cwd(), stdin: input, timeoutMs: 5_000 },
    );

    expect(result.stdout).toBe(input);
  });

  // This exercises an OS process timeout and signals, which Vitest fake timers cannot drive.
  test("includes buffered output when a command times out", async () => {
    const command = runCommand(
      process.execPath,
      [
        "-e",
        'process.stdout.write("partial stdout\\n"); process.stderr.write("partial stderr\\n"); setInterval(() => {}, 1_000);',
      ],
      { cwd: process.cwd(), timeoutMs: 500 },
    );

    await expect(command).rejects.toThrow(
      /Command timed out after 500ms:[\s\S]*stdout:\npartial stdout\n[\s\S]*stderr:\npartial stderr/,
    );
  });
});
