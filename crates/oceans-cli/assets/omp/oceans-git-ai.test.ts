import { beforeEach, expect, mock, test } from "bun:test";
import type { ExtensionAPI, ExtensionContext } from "@oh-my-pi/pi-coding-agent";
import { EventEmitter } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

type SpawnCall = {
	bin: string;
	args: string[];
	options: { detached: boolean; stdio: string[] };
	payload: Record<string, unknown>;
};
const calls: SpawnCall[] = [];
let failure: "none" | "throw" | "error" | "stdin" | "exit" = "none";

mock.module("node:child_process", () => ({
	spawn(bin: string, args: string[], options: SpawnCall["options"]) {
		if (failure === "throw") throw new Error("binary unavailable");
		const child = new EventEmitter();
		const stdin = Object.assign(new EventEmitter(), {
			end(input: string) {
				calls.push({ bin, args, options, payload: JSON.parse(input) });
				const mode = failure;
				queueMicrotask(() => {
					if (mode === "stdin") stdin.emit("error", new Error("broken pipe"));
					if (mode === "error") child.emit("error", new Error("binary unavailable"));
					else child.emit("close", mode === "exit" ? 1 : 0);
				});
			},
		});
		return Object.assign(child, { stdin, unref() {}, kill() {} });
	},
}));

// Load after mocking the child-process boundary; a static import would spawn real binaries.
const { default: oceansGitAi } = await import("./oceans-git-ai.ts");
type Handler = (event: Record<string, unknown>, ctx: ExtensionContext) => unknown;
type TestContext = {
	journal: { id: string };
	cwd: string;
	agent: { kind: string };
	model: { id: string };
	sessionManager: { getSessionId(): string; getSessionFile(): string };
};

function harness() {
	const handlers = new Map<string, Handler>();
	oceansGitAi({
		pi: { VERSION: "18.20.2" },
		on(name: string, handler: Handler) {
			handlers.set(name, handler);
		},
	} as unknown as ExtensionAPI);
	return async (name: string, event: Record<string, unknown>, ctx: TestContext) => {
		const handler = handlers.get(name);
		expect(handler).toBeDefined();
		await handler!({ type: name, ...event }, ctx as unknown as ExtensionContext);
	};
}

function context(id: string, kind = "main", cwd = tmpdir()): TestContext {
	const journal = { id };
	return {
		journal,
		cwd,
		agent: { kind },
		model: { id: "first-provider/model" },
		sessionManager: {
			getSessionId: () => journal.id,
			getSessionFile: () => join(cwd, `${journal.id}.jsonl`),
		},
	};
}

const MAIN_REQUEST = "1c8f3d6a-5bf0-4e84-8dd2-81c5d27b4da1";
const SUB_REQUEST = "54fe43ef-8d7c-4d78-96a3-3e3c37e1a204";
const UPSTREAM_REQUEST = "a8098c1a-f86e-4d77-a5e7-04b1a0b4f842";
const mappings = () => calls.filter(call => call.args.join(" ") === "hook request");

beforeEach(() => {
	calls.length = 0;
	failure = "none";
});

test("concurrent main and subagent responses map gateway ids to their own journals", async () => {
	const emit = harness();
	const main = context("journal:main");
	const sub = context("journal:sub", "sub");
	const legacyEnvironment = process.env.OCEANS_SESSION_ID;
	await Promise.all([emit("session_start", {}, main), emit("session_start", {}, sub)]);
	await Promise.all([
		emit("after_provider_response", {
			status: 200,
			headers: { "X-Oceans-Request-ID": MAIN_REQUEST, "x-request-id": UPSTREAM_REQUEST },
			requestId: UPSTREAM_REQUEST,
			metadata: { session_id: "provider-session-not-the-journal" },
		}, main),
		emit("after_provider_response", {
			status: 200,
			headers: { "x-oceans-request-id": SUB_REQUEST },
			requestId: UPSTREAM_REQUEST,
		}, sub),
	]);
	expect(mappings().map(call => call.payload)).toEqual([
		{ request_id: MAIN_REQUEST, session_id: "journal:main" },
		{ request_id: SUB_REQUEST, session_id: "journal:sub" },
	]);
	main.model.id = "rotated-provider/another-model";
	await emit("after_provider_response", {
		status: 200,
		headers: { "X-OCEANS-REQUEST-ID": SUB_REQUEST },
		requestId: "another-upstream-request",
	}, main);
	expect(mappings().at(-1)?.payload).toEqual({ request_id: SUB_REQUEST, session_id: "journal:main" });
	for (const call of mappings()) {
		expect(call.options).toEqual({ detached: true, stdio: ["pipe", "ignore", "ignore"] });
		expect(Object.keys(call.payload).sort()).toEqual(["request_id", "session_id"]);
	}
	expect(process.env.OCEANS_SESSION_ID).toBe(legacyEnvironment);
});

test("no gateway-only header means no mapping, even when generic request ids are valid UUIDs", async () => {
	const emit = harness();
	for (const headers of [{}, { "x-request-id": MAIN_REQUEST }, { "request-id": MAIN_REQUEST }]) {
		await emit("after_provider_response", { status: 200, headers, requestId: MAIN_REQUEST }, context("journal:main"));
	}
	expect(calls).toEqual([]);
});

test("invalid gateway or journal ids are ignored rather than queued or echoed", async () => {
	const emit = harness();
	for (const requestId of ["", "secret-upstream-request", MAIN_REQUEST.replaceAll("-", ""), `${MAIN_REQUEST}\n`, `${MAIN_REQUEST}, ${SUB_REQUEST}`]) {
		await emit("after_provider_response", {
			status: 200,
			headers: { "x-oceans-request-id": requestId },
			requestId: MAIN_REQUEST,
		}, context("journal:main"));
	}
	for (const sessionId of ["", "secret journal", "journal\n", "x".repeat(257)]) {
		await emit("after_provider_response", {
			status: 200,
			headers: { "x-oceans-request-id": MAIN_REQUEST },
		}, context(sessionId));
	}
	expect(calls).toEqual([]);
});

test("a current journal is read at response time without a lifecycle or provider-session cache", async () => {
	const emit = harness();
	const ctx = context("journal:first");
	await emit("after_provider_response", { status: 200, headers: { "x-oceans-request-id": MAIN_REQUEST } }, ctx);
	ctx.journal.id = "journal:branched";
	await emit("after_provider_response", { status: 200, headers: { "x-oceans-request-id": SUB_REQUEST } }, ctx);
	expect(mappings().map(call => call.payload.session_id)).toEqual(["journal:first", "journal:branched"]);
});

test("missing binaries, broken pipes and sender failures never reject the response hook", async () => {
	const emit = harness();
	for (const mode of ["throw", "error", "stdin", "exit"] as const) {
		failure = mode;
		await emit("after_provider_response", {
			status: 200,
			headers: { "x-oceans-request-id": MAIN_REQUEST },
		}, context("journal:main"));
	}
});

test("session lifecycle reporting remains journal-local through switches and branches", async () => {
	const emit = harness();
	const main = context("journal:main");
	const sub = context("journal:sub", "sub");
	await emit("session_start", {}, main);
	await emit("session_start", {}, sub);
	await emit("session_start", {}, main);
	main.journal.id = "journal:switched";
	await emit("session_switch", {}, main);
	main.journal.id = "journal:branched";
	await emit("session_branch", {}, main);
	await emit("session_shutdown", {}, sub);
	await emit("session_shutdown", {}, main);
	await emit("session_shutdown", {}, main);
	expect(calls.map(call => [call.payload.event, call.payload.session_id])).toEqual([
		["start", "journal:main"], ["start", "journal:sub"],
		["end", "journal:main"], ["start", "journal:switched"],
		["end", "journal:switched"], ["start", "journal:branched"],
		["end", "journal:sub"], ["end", "journal:branched"],
	]);
	expect(calls.every(call => call.payload.harness_version === "18.20.2")).toBe(true);
});

test("Git AI edit checkpoints still capture the pre-edit and post-edit file contents", async () => {
	const cwd = await mkdtemp(join(tmpdir(), "oceans-omp-extension-"));
	try {
		const path = join(cwd, "edited.txt");
		await writeFile(path, "before");
		const emit = harness();
		const ctx = context("journal:editing", "main", cwd);
		await emit("tool_call", { toolName: "write", toolCallId: "tool:1", input: { path } }, ctx);
		await writeFile(path, "after");
		await emit("tool_result", { toolName: "write", toolCallId: "tool:1", isError: false }, ctx);
		expect(calls.map(call => call.payload.hook_event_name)).toEqual(["before_edit", "after_edit"]);
		expect(calls[0].payload.dirty_files).toEqual({ [path]: "before" });
		expect(calls[1].payload.dirty_files).toEqual({ [path]: "after" });
		expect(calls.every(call => call.payload.session_id === "journal:editing")).toBe(true);
	} finally {
		await rm(cwd, { recursive: true, force: true });
	}
});
