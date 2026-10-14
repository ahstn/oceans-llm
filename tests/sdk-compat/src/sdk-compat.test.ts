// Feeds gateway SSE output to the official SDKs, unmodified, to show they still parse
// streams that carry `usage.oceans.cost`. The gateway test
// `sdk_compat_fixtures_match_gateway_output` checks the fixtures match gateway output.
import { readFileSync } from "node:fs";

import Anthropic from "@anthropic-ai/sdk";
import OpenAI from "openai";
import { describe, expect, it } from "vitest";

const COST = "0.0012";

function fixture(name: string): string {
  return readFileSync(new URL(`../fixtures/${name}.sse`, import.meta.url), "utf8");
}

function sseFetch(body: string): typeof fetch {
  return async () =>
    new Response(body, {
      headers: {
        "content-type": "text/event-stream; charset=utf-8",
        "x-oceans-model": "fast",
        "x-oceans-provider": "vertex",
      },
    });
}

function openai(name: string): OpenAI {
  return new OpenAI({
    apiKey: "sdk-compat",
    baseURL: "http://sdk-compat.test/v1",
    fetch: sseFetch(fixture(name)),
    maxRetries: 0,
  });
}

function anthropic(name: string): Anthropic {
  return new Anthropic({
    apiKey: "sdk-compat",
    baseURL: "http://sdk-compat.test",
    fetch: sseFetch(fixture(name)),
    maxRetries: 0,
  });
}

function oceansCost(usage: unknown): unknown {
  return (usage as { oceans?: { cost?: unknown } } | null | undefined)?.oceans?.cost;
}

describe("openai chat completions", () => {
  it("iterates chunks and exposes the cost on the usage chunk", async () => {
    const { data: stream, response } = await openai("chat_completions")
      .chat.completions.create({
        model: "fast",
        messages: [{ role: "user", content: "hi" }],
        stream: true,
        stream_options: { include_usage: true },
      })
      .withResponse();
    let text = "";
    let usage: OpenAI.CompletionUsage | null | undefined;
    for await (const chunk of stream) {
      text += chunk.choices[0]?.delta.content ?? "";
      usage = chunk.usage ?? usage;
    }
    expect(response.headers.get("x-oceans-model")).toBe("fast");
    expect(text).toBe("Hello world");
    expect(usage?.total_tokens).toBe(7);
    expect(oceansCost(usage)).toBe(COST);
  });

  it("accumulates the final completion with the stream helper", async () => {
    const completion = await openai("chat_completions")
      .chat.completions.stream({
        model: "fast",
        messages: [{ role: "user", content: "hi" }],
        stream_options: { include_usage: true },
      })
      .finalChatCompletion();
    expect(completion.choices[0]?.message.content).toBe("Hello world");
    expect(completion.usage?.total_tokens).toBe(7);
  });
});

describe("openai responses", () => {
  it("iterates events and exposes the cost on response.completed", async () => {
    const stream = await openai("responses").responses.create({
      model: "fast",
      input: "hi",
      stream: true,
    });
    let completed: OpenAI.Responses.Response | undefined;
    for await (const event of stream) {
      if (event.type === "response.completed") {
        completed = event.response;
      }
    }
    expect(completed?.output[0]).toMatchObject({
      content: [{ type: "output_text", text: "Hello world" }],
    });
    expect(completed?.usage?.total_tokens).toBe(7);
    expect(oceansCost(completed?.usage)).toBe(COST);
  });

  it("accumulates the final response with the stream helper", async () => {
    const response = await openai("responses")
      .responses.stream({ model: "fast", input: "hi" })
      .finalResponse();
    expect(response.output_text).toBe("Hello world");
    expect(response.usage?.total_tokens).toBe(7);
  });
});

describe("anthropic messages", () => {
  it("iterates events and exposes the cost on message_delta", async () => {
    const stream = await anthropic("messages").messages.create({
      model: "fast",
      max_tokens: 64,
      messages: [{ role: "user", content: "hi" }],
      stream: true,
    });
    let text = "";
    let deltaUsage: Anthropic.MessageDeltaUsage | undefined;
    for await (const event of stream) {
      if (event.type === "content_block_delta" && event.delta.type === "text_delta") {
        text += event.delta.text;
      }
      if (event.type === "message_delta") {
        deltaUsage = event.usage;
      }
    }
    expect(text).toBe("Hello world");
    expect(deltaUsage?.output_tokens).toBe(2);
    expect(oceansCost(deltaUsage)).toBe(COST);
  });

  it("accumulates the final message with the stream helper", async () => {
    const message = await anthropic("messages")
      .messages.stream({
        model: "fast",
        max_tokens: 64,
        messages: [{ role: "user", content: "hi" }],
      })
      .finalMessage();
    expect(message.content).toMatchObject([{ type: "text", text: "Hello world" }]);
    expect(message.usage.output_tokens).toBe(2);
    expect(message.stop_reason).toBe("end_turn");
  });
});
