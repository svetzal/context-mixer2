import type { Filesystem } from "./gateway.ts";
import { loadJson, saveJson } from "./json-file.ts";
import type { ConfigPaths } from "./paths.ts";
import { defaultLockFile, type InstallScope, type LockEntry, type LockFile } from "./types.ts";

const normalizeLockEntry = (entry: LockEntry): LockEntry => ({
  type: entry.type,
  version: entry.version,
  installed_at: entry.installed_at,
  source: {
    repo: entry.source.repo,
    path: entry.source.path,
  },
  source_checksum: entry.source_checksum,
  installed_checksum: entry.installed_checksum,
});

const normalizeLockFile = (lockFile: LockFile): LockFile => ({
  version: lockFile.version ?? 1,
  packages: Object.fromEntries(
    Object.entries(lockFile.packages ?? {})
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([name, entry]) => [name, normalizeLockEntry(entry)]),
  ),
});

export const loadLockFileFrom = async (targetPath: string, fs: Filesystem): Promise<LockFile> =>
  normalizeLockFile(await loadJson(targetPath, fs, defaultLockFile));

export const saveLockFileTo = async (
  lockFile: LockFile,
  targetPath: string,
  fs: Filesystem,
): Promise<void> => {
  await saveJson(normalizeLockFile(lockFile), targetPath, fs);
};

export const loadLockFile = async (
  scope: InstallScope,
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<LockFile> => await loadLockFileFrom(paths.lockPath(scope), fs);

export const saveLockFile = async (
  lockFile: LockFile,
  scope: InstallScope,
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<void> => {
  await saveLockFileTo(lockFile, paths.lockPath(scope), fs);
};

export const mutateLockFile = async <T>(
  scope: InstallScope,
  fs: Filesystem,
  paths: ConfigPaths,
  mutator: (lockFile: LockFile) => T | Promise<T>,
): Promise<T> => {
  const lockFile = await loadLockFile(scope, fs, paths);
  const result = await mutator(lockFile);
  await saveLockFile(lockFile, scope, fs, paths);
  return result;
};
