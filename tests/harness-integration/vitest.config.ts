import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    fileParallelism: false,
    globalSetup: ["./src/global-setup.ts"],
    hookTimeout: 120_000,
    // The matrix runs the three models of one harness and test at the same time.
    maxConcurrency: 3,
    maxWorkers: 1,
    // A harness command may run for four minutes, and its request logs are then awaited.
    testTimeout: 330_000,
  },
});
