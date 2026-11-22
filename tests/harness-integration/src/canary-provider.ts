import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";

import { z } from "zod";

/**
 * A local stand-in for an inference provider. It keeps every request body it receives, so a test
 * can check what left the gateway, and it answers from a script, so a test can make the "model"
 * ask for a shell command. It makes no paid calls.
 *
 * - `POST /v1/chat/completions` records the body and answers as a model, streamed or not.
 * - `POST /v1/responses` records the body and answers `CANARY_OK`, not streamed.
 * - `GET /captures?contains=<text>` returns the recorded bodies that contain `text`.
 *
 * The script reads the conversation:
 * - after a tool result, the answer is `CANARY_DONE`;
 * - when the last user message holds `<canary-shell>COMMAND</canary-shell>`, the answer is a call
 *   of the request's shell tool with that command, or of its first tool when none is named
 *   `bash` or `shell`;
 * - otherwise the answer is `CANARY_OK`.
 */
export interface CanaryProvider {
  /** The provider base URL for the gateway config, ending in `/v1`. */
  baseUrl: string;
  /** The URL of the capture query. */
  capturesUrl: string;
  close(): Promise<void>;
}

export interface CanaryCapture {
  path: string;
  /** The request body exactly as the gateway sent it. */
  body: string;
}

const CaptureListSchema = z.array(z.object({ path: z.string(), body: z.string() }));
const SHELL_TOOL_NAMES = ["bash", "shell", "Bash"];
const SHELL_DIRECTIVE = /<canary-shell>([\s\S]*?)<\/canary-shell>/;

/** Wraps a shell command so that the canary model asks the harness to run it. */
export function canaryShellDirective(command: string): string {
  return `<canary-shell>${command}</canary-shell>`;
}

/** The bodies that the canary provider received and that contain `text`. */
export async function fetchCanaryCaptures(
  capturesUrl: string,
  text: string,
): Promise<CanaryCapture[]> {
  const url = new URL(capturesUrl);
  url.searchParams.set("contains", text);
  const response = await fetch(url, { signal: AbortSignal.timeout(10_000) });
  if (!response.ok) {
    throw new Error(`Canary capture query failed with HTTP ${response.status}`);
  }
  return CaptureListSchema.parse(await response.json());
}

export async function startCanaryProvider(): Promise<CanaryProvider> {
  const captures: CanaryCapture[] = [];
  const server = createServer((request, response) => {
    handle(request, response, captures).catch((error: unknown) => {
      response.writeHead(500, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: { message: String(error) } }));
    });
  });
  const { promise, resolve, reject } = Promise.withResolvers<void>();
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
  await promise;
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return {
    baseUrl: `${origin}/v1`,
    capturesUrl: `${origin}/captures`,
    close: () =>
      new Promise((closed, failed) => {
        server.close((error) => (error ? failed(error) : closed()));
        server.closeAllConnections();
      }),
  };
}

async function handle(
  request: IncomingMessage,
  response: ServerResponse,
  captures: CanaryCapture[],
): Promise<void> {
  const url = new URL(request.url ?? "/", "http://canary.invalid");
  if (request.method === "GET" && url.pathname === "/captures") {
    const text = url.searchParams.get("contains") ?? "";
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify(captures.filter((capture) => capture.body.includes(text))));
    return;
  }
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    chunks.push(chunk as Buffer);
  }
  const body = Buffer.concat(chunks).toString("utf8");
  captures.push({ path: url.pathname, body });
  if (request.method === "POST" && url.pathname === "/v1/responses") {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify(responsesReply(body)));
    return;
  }
  if (request.method !== "POST" || url.pathname !== "/v1/chat/completions") {
    // Recorded above, so a test can see a request for an API this provider does not script.
    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: { message: `canary has no ${url.pathname}` } }));
    return;
  }
  const chat = ChatRequestSchema.parse(JSON.parse(body));
  const reply = scriptedReply(chat);
  if (chat.stream) {
    response.writeHead(200, { "cache-control": "no-cache", "content-type": "text/event-stream" });
    for (const event of streamEvents(chat.model, reply)) {
      response.write(`data: ${JSON.stringify(event)}\n\n`);
    }
    response.end("data: [DONE]\n\n");
    return;
  }
  response.writeHead(200, { "content-type": "application/json" });
  response.end(
    JSON.stringify({
      ...envelope(chat.model, "chat.completion"),
      choices: [{ index: 0, message: { role: "assistant", ...reply.message }, finish_reason: reply.finishReason }],
      usage: USAGE,
    }),
  );
}

const ChatRequestSchema = z.object({
  model: z.string(),
  stream: z.boolean().optional(),
  messages: z.array(z.object({ role: z.string(), content: z.unknown().optional() })),
  tools: z.array(z.object({ function: z.object({ name: z.string() }) })).optional(),
});
type ChatRequest = z.infer<typeof ChatRequestSchema>;

interface Reply {
  message: { content: string | null; tool_calls?: unknown[] };
  finishReason: "stop" | "tool_calls";
}

const USAGE = { prompt_tokens: 8, completion_tokens: 4, total_tokens: 12 };

function scriptedReply(chat: ChatRequest): Reply {
  const last = chat.messages.at(-1);
  if (last?.role === "tool") {
    return { message: { content: "CANARY_DONE" }, finishReason: "stop" };
  }
  const command = SHELL_DIRECTIVE.exec(last?.role === "user" ? messageText(last.content) : "")?.[1];
  if (command === undefined) {
    return { message: { content: "CANARY_OK" }, finishReason: "stop" };
  }
  const offered = chat.tools?.map((tool) => tool.function.name) ?? [];
  const name =
    SHELL_TOOL_NAMES.find((candidate) => offered.includes(candidate)) ?? offered[0] ?? "bash";
  return {
    message: {
      content: null,
      tool_calls: [
        {
          index: 0,
          id: "call_canary_shell",
          type: "function",
          function: { name, arguments: JSON.stringify({ command }) },
        },
      ],
    },
    finishReason: "tool_calls",
  };
}

/** The text of a message whose content is a string or a list of text parts. */
function messageText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((part: unknown) =>
      typeof part === "object" && part !== null && "text" in part ? String(part.text) : "",
    )
    .join("\n");
}

function responsesReply(body: string): Record<string, unknown> {
  const { model } = z.object({ model: z.string() }).parse(JSON.parse(body));
  return {
    id: "resp_canary",
    object: "response",
    created_at: Math.floor(Date.now() / 1000),
    status: "completed",
    model,
    output: [
      {
        type: "message",
        id: "msg_canary",
        status: "completed",
        role: "assistant",
        content: [{ type: "output_text", text: "CANARY_OK", annotations: [] }],
      },
    ],
    usage: { input_tokens: 8, output_tokens: 4, total_tokens: 12 },
  };
}

function envelope(model: string, object: string): Record<string, unknown> {
  return { id: "chatcmpl-canary", object, created: Math.floor(Date.now() / 1000), model };
}

function streamEvents(model: string, reply: Reply): unknown[] {
  const chunk = envelope(model, "chat.completion.chunk");
  return [
    {
      ...chunk,
      choices: [{ index: 0, delta: { role: "assistant", ...reply.message }, finish_reason: null }],
    },
    { ...chunk, choices: [{ index: 0, delta: {}, finish_reason: reply.finishReason }] },
    { ...chunk, choices: [], usage: USAGE },
  ];
}
