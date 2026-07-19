import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    fileParallelism: false,
    globalSetup: ["./src/global-setup.ts"],
    hookTimeout: 120_000,
    maxWorkers: 1,
    testTimeout: 180_000,
  },
});
