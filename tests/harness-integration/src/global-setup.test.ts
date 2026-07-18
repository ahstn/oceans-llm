import { describe, expect, test } from "vitest";

import { externalAllowlistedUser, normalizeExternalBaseUrl } from "./global-setup.js";

describe("external gateway configuration", () => {
  test("allows HTTPS and loopback HTTP gateway URLs", () => {
    expect(normalizeExternalBaseUrl("https://gateway.example.com/")).toBe(
      "https://gateway.example.com",
    );
    expect(normalizeExternalBaseUrl("http://127.0.0.1:8080/")).toBe(
      "http://127.0.0.1:8080",
    );
    expect(normalizeExternalBaseUrl("http://[::1]:8080/")).toBe("http://[::1]:8080");
  });

  test("rejects cleartext remote gateways and ambiguous base URLs", () => {
    expect(() => normalizeExternalBaseUrl("http://gateway.example.com")).toThrow(/HTTPS/);
    expect(() => normalizeExternalBaseUrl("https://gateway.example.com?tenant=test")).toThrow(
      /query string or fragment/,
    );
  });

  test("requires both external allowlist settings or neither", () => {
    expect(externalAllowlistedUser(undefined, undefined)).toBeUndefined();
    expect(externalAllowlistedUser("gwk_user.secret", "allowlisted-model")).toEqual({
      apiKey: "gwk_user.secret",
      model: "allowlisted-model",
    });
    expect(() => externalAllowlistedUser("gwk_user.secret", undefined)).toThrow(
      /must be configured together/,
    );
    expect(() => externalAllowlistedUser(undefined, "allowlisted-model")).toThrow(
      /must be configured together/,
    );
  });
});
