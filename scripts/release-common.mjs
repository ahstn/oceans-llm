#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

export const repoRoot = process.cwd();
export const stableTagPattern = /^v\d+\.\d+\.\d+$/;

function formatCommand(command, args) {
  return [command, ...args].join(" ");
}

export function runCommand(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: options.env ? { ...process.env, ...options.env } : process.env,
    encoding: "utf8",
    input: options.input,
    stdio: options.stdio ?? "pipe",
  });

  if (options.allowFailure || result.status === 0) {
    return result;
  }

  const stderr = (result.stderr ?? "").trim();
  const stdout = (result.stdout ?? "").trim();
  const details = stderr || stdout || `exit code ${result.status}`;
  throw new Error(`${formatCommand(command, args)} failed: ${details}`);
}

export function git(args, options = {}) {
  return runCommand("git", args, options);
}

export function gitStdout(args, options = {}) {
  return git(args, options).stdout.trim();
}

export function getCurrentBranch() {
  return gitStdout(["rev-parse", "--abbrev-ref", "HEAD"]);
}

export function getHeadSha() {
  return gitStdout(["rev-parse", "HEAD"]);
}

export function getShortHeadSha() {
  return gitStdout(["rev-parse", "--short", "HEAD"]);
}

export function listStableTags() {
  return gitStdout(["tag", "--sort=-version:refname", "--merged", "HEAD"])
    .split("\n")
    .map((tag) => tag.trim())
    .filter((tag) => stableTagPattern.test(tag));
}

export function getLatestStableTag() {
  return listStableTags()[0] ?? null;
}

function parseCommitStream(output) {
  return output
    .split("\u0000")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [subject, ...bodyLines] = entry.split("\n");
      return {
        subject: subject.trim(),
        body: bodyLines.join("\n").trim(),
        raw: entry,
      };
    });
}

export function getCommitsSinceTag(lastTag) {
  const args = ["log", "--format=%s%n%b%x00"];
  if (lastTag) {
    args.push(`${lastTag}..HEAD`);
  }

  return parseCommitStream(git(args).stdout);
}

export function analyzeReleaseType(commits) {
  let releaseType = null;

  for (const commit of commits) {
    if (
      /^BREAKING[ -]CHANGES?:/m.test(commit.body) ||
      /^[a-z]+(?:\([A-Za-z0-9_./-]+\))?!: /.test(commit.subject)
    ) {
      return "major";
    }

    if (!releaseType && /^feat(?:\([A-Za-z0-9_./-]+\))?: /.test(commit.subject)) {
      releaseType = "minor";
      continue;
    }

    if (!releaseType && /^fix(?:\([A-Za-z0-9_./-]+\))?: /.test(commit.subject)) {
      releaseType = "patch";
    }
  }

  return releaseType;
}

export function bumpVersion(lastTag, releaseType) {
  const match = /^v(\d+)\.(\d+)\.(\d+)$/.exec(lastTag);
  if (!match) {
    throw new Error(`invalid stable tag: ${lastTag}`);
  }

  let major = Number(match[1]);
  let minor = Number(match[2]);
  let patch = Number(match[3]);

  switch (releaseType) {
    case "major":
      major += 1;
      minor = 0;
      patch = 0;
      break;
    case "minor":
      minor += 1;
      patch = 0;
      break;
    case "patch":
      patch += 1;
      break;
    default:
      throw new Error(`unsupported release type: ${releaseType}`);
  }

  return `${major}.${minor}.${patch}`;
}

export function getInitialVersion(releaseType) {
  if (releaseType === "patch") {
    return "0.0.1";
  }

  return "0.1.0";
}

export function getMajorMinor(version) {
  return version.split(".").slice(0, 2).join(".");
}

export function makeTag(version) {
  return `v${version}`;
}

export function analyzeRepository() {
  const lastStableTag = getLatestStableTag();
  const commits = getCommitsSinceTag(lastStableTag);
  const releaseType = analyzeReleaseType(commits);

  if (!releaseType) {
    return {
      commits,
      hasRelease: false,
      majorMinor: null,
      previousTag: lastStableTag,
      releaseSha: getHeadSha(),
      shortSha: getShortHeadSha(),
      tag: null,
      version: null,
    };
  }

  const version = lastStableTag
    ? bumpVersion(lastStableTag, releaseType)
    : getInitialVersion(releaseType);

  return {
    commits,
    hasRelease: true,
    majorMinor: getMajorMinor(version),
    previousTag: lastStableTag,
    releaseSha: getHeadSha(),
    releaseType,
    shortSha: getShortHeadSha(),
    tag: makeTag(version),
    version,
  };
}

export function cargoPrepare(version) {
  const cargoRelease = require("@semantic-release-cargo/semantic-release-cargo");
  cargoRelease.prepare({}, { nextRelease: { version } });
}
