import { describe, expect, test } from "vitest";

import { parseGeneratedOpenCodeConfig } from "./opencode.js";

function generatedConfig(baseURL: string) {
  return {
    model: "oceans-llm-openai-compatible/gpt-6-luna",
    providers: {
      "oceans-llm-openai-compatible": {
        env: ["OCEANS_LLM_API_KEY"],
        models: { "gpt-6-luna": {} },
        package: "@opencode/ai/providers/openai-compatible",
        settings: { baseURL },
      },
    },
  };
}

describe("generated OpenCode config", () => {
  test("accepts HTTPS and loopback HTTP provider endpoints", () => {
    expect(() =>
      parseGeneratedOpenCodeConfig(generatedConfig("https://gateway.example.com/v1")),
    ).not.toThrow();
    expect(() =>
      parseGeneratedOpenCodeConfig(generatedConfig("http://127.0.0.1:3000/v1")),
    ).not.toThrow();
  });

  test("rejects a cleartext remote endpoint before the API key is supplied", () => {
    expect(() =>
      parseGeneratedOpenCodeConfig(generatedConfig("http://gateway.example.com/v1")),
    ).toThrow(/oceans-llm-openai-compatible baseURL must use HTTPS/);
  });
});
