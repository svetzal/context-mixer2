import { createHash } from "node:crypto";
import path from "node:path/posix";

import type { Filesystem } from "./gateway.ts";

const transientNames = new Set(["node_modules", "__pycache__", ".git", ".DS_Store"]);

export interface ChecksumEntry {
  relPath: string;
  bytes: Uint8Array;
}

const comparePathKeys = (left: string, right: string): number =>
  left < right ? -1 : left > right ? 1 : 0;

export const normalizeRelPath = (relPath: string): string =>
  relPath.replaceAll("\\", "/").replace(/^\.\/+/u, "");

export const relPathComponents = (relPath: string): string[] =>
  normalizeRelPath(relPath)
    .split("/")
    .filter((component) => component.length > 0);

export const relPathKey = (relPath: string): string => relPathComponents(relPath).join("/");

export const isCanonicalRelPath = (relPath: string): boolean =>
  relPathComponents(relPath).every((component) => {
    if (component.startsWith(".")) {
      return false;
    }

    if (transientNames.has(component)) {
      return false;
    }

    return !component.toLowerCase().endsWith(".pyc");
  });

export const checksumInMemory = (
  entries: Iterable<{ relPath: string; bytes: Uint8Array }>,
): string => {
  const hasher = createHash("sha256");
  for (const entry of entries) {
    hasher.update(relPathKey(entry.relPath), "utf8");
    hasher.update(entry.bytes);
  }

  return `sha256:${hasher.digest("hex")}`;
};

const collectFilesRecursive = async (
  rootDir: string,
  currentDir: string,
  fs: Filesystem,
): Promise<ChecksumEntry[]> => {
  if (!(await fs.exists(currentDir))) {
    return [];
  }

  const entries = await fs.listDir(currentDir);
  const files: ChecksumEntry[] = [];

  for (const entry of entries) {
    const absolutePath = path.join(currentDir, entry.fileName);
    if (entry.isDirectory) {
      files.push(...(await collectFilesRecursive(rootDir, absolutePath, fs)));
      continue;
    }

    const relPath = path.relative(rootDir, absolutePath);
    files.push({
      relPath,
      bytes: await fs.read(absolutePath),
    });
  }

  return files;
};

export const checksumDir = async (rootDir: string, fs: Filesystem): Promise<string> => {
  const entries = await collectFilesRecursive(rootDir, rootDir, fs);
  const canonicalEntries = entries
    .filter((entry) => isCanonicalRelPath(entry.relPath))
    .sort((left, right) => comparePathKeys(relPathKey(left.relPath), relPathKey(right.relPath)));

  return checksumInMemory(canonicalEntries);
};
