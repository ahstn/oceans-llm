import { describe, expect, test } from "bun:test";
import { buildOverrides, envInputReader, parseInputs } from "./input";

describe("input parsing", () => {
  test("parses required inputs, booleans, integers, and default token", () => {
    const inputs = parseInputs(envInputReader({
      "INPUT_OCEANS-URL": "https://oceans.example.test/",
      "INPUT_OCEANS-API-KEY": "key",
      "INPUT_INLINE-REVIEW": "true",
      "INPUT_TIMEOUT-MINUTES": "7",
      "INPUT_MAX-INLINE-COMMENTS": "12",
      "INPUT_DRY-RUN": "yes",
      "INPUT_GITHUB-TOKEN": "gh-token",
      GITHUB_TOKEN: "gh-token"
    }));

    expect(inputs.oceansUrl).toBe("https://oceans.example.test");
    expect(inputs.inlineReview).toBe(true);
    expect(inputs.timeoutMinutes).toBe(7);
    expect(inputs.maxInlineComments).toBe(12);
    expect(inputs.dryRun).toBe(true);
    expect(inputs.githubToken).toBe("gh-token");
  });

  test("builds only explicit config overrides", () => {
    const overrides = buildOverrides({
      oceansUrl: "https://oceans.example.test",
      oceansApiKey: "key",
      modelId: "gpt-5",
      inlineReview: false,
      timeoutMinutes: 20,
      dryRun: false,
      debug: false
    });

    expect(overrides).toEqual({
      model_id: "gpt-5",
      inline_review_enabled: false
    });
  });
});
