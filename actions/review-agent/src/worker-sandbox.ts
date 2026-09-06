import { existsSync, realpathSync } from 'node:fs'
import { dirname } from 'node:path'

// The root starts empty. In particular, never mount procfs: its environ files
// would expose model credentials to read/fetch tools, even in a PID namespace.
export function sandboxCommand(
  actionRoot: string,
  workspace: string,
  tempDir: string,
  workerArgs: string[],
): { command: string; args: string[] } {
  if (process.platform !== 'linux' || !existsSync('/usr/bin/bwrap')) {
    throw new Error('The review worker requires Linux with /usr/bin/bwrap (bubblewrap) installed')
  }
  const args = [
    '--unshare-user',
    '--unshare-pid',
    '--unshare-ipc',
    '--unshare-uts',
    '--die-with-parent',
    '--new-session',
    '--cap-drop',
    'ALL',
  ]
  for (const path of [
    '/usr',
    '/bin',
    '/lib',
    '/lib64',
    '/etc/ssl',
    '/etc/resolv.conf',
    '/etc/hosts',
    '/etc/ld.so.cache',
  ]) {
    if (existsSync(path)) args.push('--ro-bind', path, path)
  }
  for (const path of new Set([actionRoot, workspace, dirname(process.execPath)])) {
    const resolved = realpathSync(path)
    args.push('--ro-bind', resolved, path)
  }
  args.push(
    '--dev',
    '/dev',
    '--dir',
    '/proc',
    '--bind',
    realpathSync(tempDir),
    tempDir,
    '--chdir',
    tempDir,
    '--',
    process.execPath,
    ...workerArgs,
  )
  return { command: '/usr/bin/bwrap', args }
}
