/**
 * JavaScript that every generated guardrail hook starts with. The harnesses load their hooks from
 * files, so these helpers are emitted as source text, not imported.
 *
 * - `guardShell(command)` asks Oceans guardrails about a shell command. It returns the command to
 *   run, and throws when the guardrail denies the command, fails, or returns an invalid decision.
 * - `confineShell(command)` wraps a command in an OS sandbox that can see only the workspace. It
 *   returns `null` on a platform without a sandbox.
 * - `workspacePathError(path)` returns why a file-tool path is not a plain path in the workspace,
 *   or `undefined` when the path is safe.
 */
export function sandboxHelpersSource(workspace: string): string {
  return `import { isAbsolute, relative, resolve, sep } from "node:path";

const workspace = ${JSON.stringify(workspace)};
const guardrailUrl = new URL("/api/v1/guardrails/evaluate", process.env.OCEANS_BASE_URL).toString();
const guardrailTimeoutMs = Number(process.env.OCEANS_GUARDRAIL_TIMEOUT_MS ?? "2000");

function validateGuardrailDecision(value) {
  if (
    value === null ||
    typeof value !== "object" ||
    typeof value.allowed !== "boolean" ||
    typeof value.transformed !== "boolean" ||
    typeof value.decision_id !== "string" ||
    value.decision_id.length === 0 ||
    (!value.allowed && (typeof value.reason_code !== "string" || value.reason_code.length === 0))
  ) {
    throw new Error("Guardrail evaluation returned an invalid response");
  }
  return value;
}

async function guardShell(command) {
  const response = await fetch(guardrailUrl, {
    signal: AbortSignal.timeout(guardrailTimeoutMs),
    method: "POST",
    headers: {
      authorization: "Bearer " + process.env.OCEANS_API_KEY,
      "content-type": "application/json",
    },
    body: JSON.stringify({ tool_name: "bash", command }),
  });
  if (!response.ok) {
    throw new Error("Guardrail evaluation failed with HTTP " + response.status);
  }
  const decision = validateGuardrailDecision(await response.json());
  console.error("oceans_guardrail_decision_id=" + decision.decision_id);
  if (!decision.allowed) {
    throw new Error("Oceans guardrail denied shell execution: " + decision.reason_code);
  }
  if (!decision.transformed) {
    return command;
  }
  if (typeof decision.output_command !== "string") {
    throw new Error("Oceans guardrail returned an invalid shell transformation");
  }
  return decision.output_command;
}

function shellQuote(value) {
  return "'" + value.replaceAll("'", "'\\"'\\"'") + "'";
}

function confineShell(command) {
  if (process.platform === "linux") {
    return "bwrap --die-with-parent --unshare-all --new-session --tmpfs / --proc /proc --dev /dev --tmpfs /tmp --ro-bind-try /usr /usr --ro-bind-try /bin /bin --ro-bind-try /lib /lib --ro-bind-try /lib64 /lib64 --bind " +
      shellQuote(workspace) + " /workspace --chdir /workspace /bin/sh -c " + shellQuote(command);
  }
  if (process.platform === "darwin") {
    const profile = '(version 1)(deny default)(allow process*)(allow sysctl-read)(allow mach-lookup)(allow file-read* (subpath "/System") (subpath "/usr") (subpath "/bin") (subpath "/dev") (subpath ' +
      JSON.stringify(workspace) + '))(allow file-write* (subpath ' + JSON.stringify(workspace) + '))';
    return "/usr/bin/sandbox-exec -p " + shellQuote(profile) + " /bin/sh -c " + shellQuote(command);
  }
  return null;
}

// Path syntax that the harnesses resolve to something other than a plain relative path: "~" (home),
// "@" (Pi and OMP strip it as a mention prefix), and URLs or internal schemes such as xd://,
// local://, agent:// and proc://.
const harnessPathSyntax = /^(?:[~@]|[a-z][a-z0-9+.-]*:\\/\\/)/i;
// OMP accepts a hashline header that the model copied from a read, "[path#TAG]", in place of a path.
const hashlineHeader = /^\\[(.+)#[0-9a-f]{4}\\]$/i;

function workspacePathError(target) {
  if (typeof target !== "string" || target.trim() === "") {
    return "File path is missing";
  }
  const unwrapped = hashlineHeader.exec(target.trim())?.[1];
  for (const candidate of unwrapped === undefined ? [target] : [target, unwrapped]) {
    const path = candidate.trim();
    if (harnessPathSyntax.test(path) || !isWithinWorkspace(path)) {
      return "File access outside the harness workspace is disabled";
    }
  }
  return undefined;
}

function isWithinWorkspace(target) {
  const relativePath = relative(workspace, resolve(workspace, target));
  return (
    relativePath === "" ||
    (relativePath !== ".." && !relativePath.startsWith(".." + sep) && !isAbsolute(relativePath))
  );
}
`;
}
