// Oceans attribution extension for Oh My Pi (OMP), installed by `oceans hook install --omp`.
//
// - Marks OMP file edits for Git AI by driving `git-ai checkpoint pi` before and after each
//   mutating tool call (git-ai has no OMP preset yet; its Pi preset accepts the same payload).
// - Reports session start and end to `oceans hook omp`, which queues them for the gateway.
// - Keeps `OCEANS_SESSION_ID` set to the main session's id. OMP resolves provider header
//   values that name an environment variable on every request, so an Oceans provider with
//   `headers: { session_id: OCEANS_SESSION_ID }` tells the gateway which session each Chat
//   Completions or Responses request belongs to. Configure that header only where this
//   extension is installed: without it, OMP sends the variable's name as the value.
//
// Both binaries are optional: a missing binary makes the matching hook a no-op.
import type { ExtensionAPI, ExtensionContext } from "@oh-my-pi/pi-coding-agent";
import { type ChildProcess, spawn } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { isAbsolute, resolve } from "node:path";

const OCEANS_BIN = "__OCEANS_BIN__";
const GIT_AI_BIN = "__GIT_AI_BIN__";
const SESSION_ENV = "OCEANS_SESSION_ID";
const CHECKPOINT_TIMEOUT_MS = 10_000;
// `dirty_files` is an optional pre-edit snapshot. Larger files are left out of it, as are
// non-regular files such as FIFOs, which a read would block on.
const MAX_DIRTY_FILE_BYTES = 8 * 1024 * 1024;

type CanonicalTool = "edit" | "write" | "bash";
type HookEvent = "before_edit" | "after_edit" | "before_command" | "after_command";

type PendingCall = { tool: CanonicalTool; rawTool: string; filepaths: string[]; input: unknown };

const EDIT_TOOLS: Record<string, CanonicalTool> = {
	edit: "edit",
	apply_patch: "edit",
	write: "write",
};

// Header forms across OMP edit modes: apply_patch, sloppy and hashline.
const PATCH_FILE_HEADER = /^\*\*\* (?:Add|Update|Delete|Edit) File: (.+?)(?: all)?\s*$/gm;
const PATCH_MOVE_HEADER = /^\*\*\* Move to: (.+?)\s*$/gm;
const HASHLINE_HEADER = /^\[([^\]\n#]+)#[^\]\n]*\]\s*$/gm;
const HASHLINE_MOVE = /^MV\s+(.+?)\s*$/gm;

function collect(text: string, pattern: RegExp, out: string[]): void {
	for (const match of text.matchAll(pattern)) {
		const value = (match[1] ?? "").trim();
		const quoted = value.length >= 2 && value.startsWith('"') && value.endsWith('"');
		out.push(quoted ? value.slice(1, -1) : value);
	}
}

/** Every file path an edit/write call can touch, in any OMP edit mode. */
function toolPaths(input: unknown): string[] {
	const raw: string[] = [];
	const record = (input ?? {}) as Record<string, unknown>;
	for (const key of ["path", "file_path", "filePath"]) {
		const value = record[key];
		if (typeof value === "string") raw.push(value);
	}
	if (Array.isArray(record.edits)) {
		for (const edit of record.edits) {
			const rename = (edit as Record<string, unknown> | null)?.rename;
			if (typeof rename === "string") raw.push(rename);
		}
	}
	if (typeof record.input === "string") {
		collect(record.input, PATCH_FILE_HEADER, raw);
		collect(record.input, PATCH_MOVE_HEADER, raw);
		collect(record.input, HASHLINE_HEADER, raw);
		collect(record.input, HASHLINE_MOVE, raw);
	}
	return raw.filter(path => path.length > 0 && !path.includes("://"));
}

function absolutePaths(cwd: string, paths: string[]): string[] {
	return [...new Set(paths.map(path => (isAbsolute(path) ? path : resolve(cwd, path))))];
}

async function dirtyFiles(paths: string[]): Promise<Record<string, string>> {
	const files: Record<string, string> = {};
	await Promise.all(
		paths.map(async path => {
			const info = await stat(path).catch(() => null);
			// A missing file is about to be created, so its content before the edit is empty.
			if (!info) {
				files[path] = "";
				return;
			}
			if (!info.isFile() || info.size > MAX_DIRTY_FILE_BYTES) return;
			const content = await readFile(path, "utf8").catch(() => null);
			if (content !== null) files[path] = content;
		}),
	);
	return files;
}

/** Runs `bin args` with `stdin`, never rejecting; resolves when the child exits or times out. */
function run(bin: string, args: string[], stdin: string, detached: boolean): Promise<void> {
	const { promise, resolve: done } = Promise.withResolvers<void>();
	let child: ChildProcess;
	try {
		child = spawn(bin, args, { stdio: ["pipe", "ignore", "ignore"], detached });
	} catch {
		done();
		return promise;
	}
	child.stdin?.on("error", () => {});
	child.stdin?.end(stdin);
	if (detached) {
		// Session hooks only queue locally; don't hold the session open for them.
		child.on("error", () => {});
		child.unref();
		done();
		return promise;
	}
	const timer = setTimeout(() => {
		child.kill();
		done();
	}, CHECKPOINT_TIMEOUT_MS);
	timer.unref();
	const finish = () => {
		clearTimeout(timer);
		done();
	};
	child.on("error", finish);
	child.on("close", finish);
	return promise;
}

export default function oceansGitAi(pi: ExtensionAPI) {
	const pending = new Map<string, PendingCall>();
	const exports: unknown = pi.pi;
	const version = exports && typeof exports === "object" && "VERSION" in exports ? exports.VERSION : null;
	const harnessVersion = typeof version === "string" ? version : null;

	const checkpoint = (event: HookEvent, call: PendingCall, ctx: ExtensionContext, toolCallId: string, extra: object) => {
		const sessionId = ctx.sessionManager.getSessionId();
		if (!sessionId) return Promise.resolve();
		const payload = {
			hook_event_name: event,
			session_id: sessionId,
			session_path: ctx.sessionManager.getSessionFile() ?? "",
			cwd: ctx.cwd,
			model: ctx.model?.id ?? "",
			tool_name: call.tool,
			tool_name_raw: call.rawTool,
			tool_use_id: toolCallId,
			...extra,
		};
		return run(GIT_AI_BIN, ["checkpoint", "pi", "--hook-input", "stdin"], JSON.stringify(payload), false);
	};

	const reportSession = (event: "start" | "end", ctx: ExtensionContext) => {
		const sessionId = ctx.sessionManager.getSessionId();
		if (!sessionId) return Promise.resolve();
		const payload = { session_id: sessionId, cwd: ctx.cwd, event, harness_version: harnessVersion };
		return run(OCEANS_BIN, ["hook", "omp"], JSON.stringify(payload), true);
	};

	// Subagents run in this process and share its environment. Only the main session sets the
	// variable, so a subagent's Chat Completions and Responses requests count for the main
	// session; over Messages, OMP sends each session's own id.
	const exportSession = (ctx: ExtensionContext) => {
		if (ctx.agent?.kind === "sub") return;
		const sessionId = ctx.sessionManager.getSessionId();
		if (sessionId) process.env[SESSION_ENV] = sessionId;
	};

	pi.on("session_start", async (_event, ctx) => {
		exportSession(ctx);
		await reportSession("start", ctx);
	});

	pi.on("session_before_switch", async (_event, ctx) => {
		await reportSession("end", ctx);
	});

	pi.on("session_switch", async (_event, ctx) => {
		exportSession(ctx);
		await reportSession("start", ctx);
	});

	pi.on("session_branch", async (_event, ctx) => {
		exportSession(ctx);
	});

	pi.on("session_shutdown", async (_event, ctx) => {
		await reportSession("end", ctx);
	});

	pi.on("tool_call", async (event, ctx) => {
		if (event.toolName === "bash") {
			const call: PendingCall = { tool: "bash", rawTool: "bash", filepaths: [], input: event.input };
			pending.set(event.toolCallId, call);
			await checkpoint("before_command", call, ctx, event.toolCallId, {});
			return;
		}
		const tool = EDIT_TOOLS[event.toolName];
		if (!tool) return;
		const filepaths = absolutePaths(ctx.cwd, toolPaths(event.input));
		if (filepaths.length === 0) return;
		const call: PendingCall = { tool, rawTool: event.toolName, filepaths, input: event.input };
		pending.set(event.toolCallId, call);
		await checkpoint("before_edit", call, ctx, event.toolCallId, {
			will_edit_filepaths: filepaths,
			dirty_files: await dirtyFiles(filepaths),
			tool_input: event.input,
		});
	});

	pi.on("tool_result", async (event, ctx) => {
		const call = pending.get(event.toolCallId);
		if (!call) return;
		pending.delete(event.toolCallId);
		// A failed call can still have changed files (a bash script that writes and then fails, a
		// partly applied patch), so it gets its after checkpoint too.
		if (call.tool === "bash") {
			await checkpoint("after_command", call, ctx, event.toolCallId, {});
			return;
		}
		await checkpoint("after_edit", call, ctx, event.toolCallId, {
			edited_filepaths: call.filepaths,
			dirty_files: await dirtyFiles(call.filepaths),
			tool_input: call.input,
		});
	});
}
