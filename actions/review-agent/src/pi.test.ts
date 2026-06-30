import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { cleanupPiInvocation, createFakePiBinary, invokePi, preparePiInvocation } from "./pi";

describe("Pi invocation", () => {
  test("uses temp files and reads the result artifact", () => {
    const tempDir = mkdtempSync(join(tmpdir(), "oceans-fake-pi-"));
    const piPath = join(tempDir, "pi");
    createFakePiBinary(piPath, {
      findings: [{ path: "src/main.ts", line: 1, message: "Check", severity: "low" }],
      metrics: { files_changed: 1 }
    });

    const invocation = preparePiInvocation({
      piBinary: piPath,
      context: {
        repository: { provider: "github", owner: "octo", name: "repo", full_name: "octo/repo" },
        pullRequest: {
          pr_number: 1,
          head_repository_full_name: "octo/repo",
          base_repository_full_name: "octo/repo",
          is_draft: false
        }
      },
      effectiveConfig: { model_id: "gpt-5" }
    });
    expect(invocation.args).toContain("--context");
    const result = invokePi(invocation, 1);
    expect(result.findings).toHaveLength(1);
    cleanupPiInvocation(invocation);
    expect(existsSync(invocation.tempDir)).toBe(false);
  });
});
