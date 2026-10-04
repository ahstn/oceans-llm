#!/usr/bin/env node

import { execFile } from 'node:child_process';
import { access, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const repoRoot = fileURLToPath(new URL('../', import.meta.url));

// This is an explicit local import workflow. Normal gateway startup does not call it.
export async function importBundledSkills({
  url,
  apiKey,
  namespace,
  executable = process.env.OCEANS_CLI_BIN ?? path.join(
    process.env.CARGO_TARGET_DIR ?? path.join(repoRoot, 'target'),
    'debug',
    process.platform === 'win32' ? 'oceans.exe' : 'oceans',
  ),
  directory = path.join(repoRoot, 'bundled-skills'),
}) {
  if (!apiKey || !namespace) {
    throw new Error('Set OCEANS_API_KEY to a user-owned key and OCEANS_SKILLS_NAMESPACE to its owner namespace');
  }

  async function cli(...args) {
    try {
      const { stdout } = await execFileAsync(executable, ['--url', url, '--json', 'skills', ...args], {
        env: { ...process.env, OCEANS_API_KEY: apiKey },
        timeout: 60_000,
        maxBuffer: 4 * 1024 * 1024,
      });
      return JSON.parse(stdout);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      throw new Error(message.replaceAll(apiKey, '[redacted]'));
    }
  }

  const existing = await cli('namespace');
  if (existing && existing.handle !== namespace) {
    throw new Error(`The current user owns namespace ${existing.handle}; requested ${namespace}`);
  }
  if (!existing) {
    await cli('namespace', namespace);
  }

  const skills = [];
  const entries = await readdir(directory, { withFileTypes: true });
  for (const entry of entries.sort((left, right) => left.name.localeCompare(right.name))) {
    if (!entry.isDirectory()) continue;
    const source = path.join(directory, entry.name);
    try {
      await access(path.join(source, 'SKILL.md'));
    } catch (error) {
      if (error.code === 'ENOENT') continue;
      throw error;
    }
    const result = await cli('upload', source, '--skip-unchanged');
    const detail = result.unchanged ? result.detail : result;
    const selectedVersion = result.unchanged ? result.version : result.uploaded_version;
    const version = detail.versions.find((version) => version.version === selectedVersion);
    if (!version) throw new Error(`The upload result for ${entry.name} has no selected version`);
    skills.push({
      name: detail.skill.name,
      id: detail.skill.id,
      version: version.version,
      unchanged: result.unchanged === true,
      sha256: version.sha256,
    });
  }
  if (skills.length === 0) throw new Error(`No bundled skills found in ${directory}`);
  return { namespace, skills };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const result = await importBundledSkills({
      url: process.env.OCEANS_URL ?? 'http://127.0.0.1:8080',
      apiKey: process.env.OCEANS_API_KEY,
      namespace: process.env.OCEANS_SKILLS_NAMESPACE,
    });
    console.log(JSON.stringify(result, null, 2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
