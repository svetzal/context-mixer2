import { mkdir, mkdtemp, readdir, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import posixPath from "node:path/posix";

import type { Clock, DirEntry, Filesystem } from "../src/gateway.ts";
import { ConfigPaths } from "../src/paths.ts";
import { lockFileName, PLATFORM_VALUES } from "../src/platform.ts";
import type {
  InstallPlan,
  Report,
  TargetAction,
  TargetOutcome,
  TargetPlan,
} from "../src/skill-installer.ts";
import type { InstallScope } from "../src/types.ts";

export const FIXED_TIMESTAMP = "2026-07-05T12:00:00+00:00";
export const VIRTUAL_HOME = "/home/testuser";
export const VIRTUAL_CONFIG_ROOT = "/home/testuser/.config/context-mixer";
export const VIRTUAL_PROJECT_ROOT = "/project";

export class FixedClock implements Clock {
  public now(): Date {
    return new Date(FIXED_TIMESTAMP);
  }
}

export class VirtualFilesystem implements Filesystem {
  public readonly rootDir: string;

  public constructor(rootDir: string) {
    this.rootDir = rootDir;
  }

  public async exists(targetPath: string): Promise<boolean> {
    try {
      await stat(this.toActualPath(targetPath));
      return true;
    } catch {
      return false;
    }
  }

  public async read(targetPath: string): Promise<Uint8Array> {
    return await readFile(this.toActualPath(targetPath));
  }

  public async readText(targetPath: string): Promise<string> {
    return await readFile(this.toActualPath(targetPath), "utf8");
  }

  public async write(targetPath: string, content: string): Promise<void> {
    await writeFile(this.toActualPath(targetPath), content, "utf8");
  }

  public async writeBytes(targetPath: string, content: Uint8Array): Promise<void> {
    await writeFile(this.toActualPath(targetPath), content);
  }

  public async createDirAll(targetPath: string): Promise<void> {
    await mkdir(this.toActualPath(targetPath), { recursive: true });
  }

  public async removeDirAll(targetPath: string): Promise<void> {
    await rm(this.toActualPath(targetPath), { recursive: true, force: true });
  }

  public async rename(from: string, to: string): Promise<void> {
    await rename(this.toActualPath(from), this.toActualPath(to));
  }

  public async listDir(targetPath: string): Promise<DirEntry[]> {
    const entries = await readdir(this.toActualPath(targetPath), { withFileTypes: true });
    return entries.map((entry) => ({
      fileName: entry.name,
      isDirectory: entry.isDirectory(),
    }));
  }

  public async isDirectory(targetPath: string): Promise<boolean> {
    try {
      return (await stat(this.toActualPath(targetPath))).isDirectory();
    } catch {
      return false;
    }
  }

  public toActualPath(virtualPath: string): string {
    const normalized = virtualPath.startsWith("/")
      ? virtualPath.slice(1)
      : posixPath.join("project", virtualPath);
    return path.join(this.rootDir, normalized);
  }

  public async snapshotFiles(): Promise<Map<string, Uint8Array>> {
    const snapshot = new Map<string, Uint8Array>();
    await collectActualFiles(this.rootDir, this.rootDir, snapshot);
    return new Map(
      [...snapshot.entries()].map(([actualPath, bytes]) => [this.toVirtualPath(actualPath), bytes]),
    );
  }

  private toVirtualPath(actualPath: string): string {
    const relative = path.relative(this.rootDir, actualPath).replaceAll("\\", "/");
    return `/${relative}`;
  }
}

const collectActualFiles = async (
  rootDir: string,
  currentDir: string,
  output: Map<string, Uint8Array>,
): Promise<void> => {
  const entries = await readdir(currentDir, { withFileTypes: true }).catch(() => undefined);
  if (entries === undefined) {
    return;
  }

  for (const entry of entries) {
    const entryPath = path.join(currentDir, entry.name);
    if (entry.isDirectory()) {
      await collectActualFiles(rootDir, entryPath, output);
      continue;
    }

    output.set(entryPath, await readFile(entryPath));
  }
};

export const makeTempFilesystem = async (): Promise<{
  fs: VirtualFilesystem;
  paths: ConfigPaths;
  cleanup: () => Promise<void>;
}> => {
  const rootDir = await mkdtemp(path.join(os.tmpdir(), "cmx-core-ts-"));
  const fs = new VirtualFilesystem(rootDir);
  const paths = new ConfigPaths({
    configDir: VIRTUAL_CONFIG_ROOT,
    homeDir: VIRTUAL_HOME,
    platform: "claude",
    projectRoot: VIRTUAL_PROJECT_ROOT,
  });

  return {
    fs,
    paths,
    cleanup: async () => {
      await rm(rootDir, { recursive: true, force: true });
    },
  };
};

export const normalizedPathString = (targetPath: string): string => {
  const normalized = targetPath.replaceAll("\\", "/");
  if (!normalized.startsWith("/")) {
    return normalized;
  }

  return `//${normalized.slice(1)}`;
};

export const normalizedFixturePath = (targetPath: string): string =>
  targetPath.startsWith("/") ? targetPath.slice(1) : posixPath.join("project", targetPath);

const decodeBytes = (bytes: Uint8Array): string => new TextDecoder().decode(bytes);

export const snapshotTree = async (
  fs: VirtualFilesystem,
  paths: ConfigPaths,
  scope: InstallScope,
): Promise<Record<string, string>> => {
  const lockPaths = new Set(
    PLATFORM_VALUES.map((platform) => paths.withPlatform(platform).lockPath(scope)),
  );
  const files = await fs.snapshotFiles();
  const entries: Array<[string, string]> = [...files.entries()]
    .filter(([virtualPath]) => !lockPaths.has(virtualPath))
    .map(([virtualPath, bytes]) => [normalizedFixturePath(virtualPath), decodeBytes(bytes)]);
  entries.sort((left, right) => left[0].localeCompare(right[0]));
  return Object.fromEntries(entries);
};

export const snapshotLocks = async (
  fs: Filesystem,
  paths: ConfigPaths,
  scope: InstallScope,
): Promise<Record<string, unknown>> => {
  const locks: Record<string, unknown> = {};
  for (const platform of PLATFORM_VALUES) {
    const lockPath = paths.withPlatform(platform).lockPath(scope);
    if (!(await fs.exists(lockPath))) {
      continue;
    }

    locks[lockFileName(platform)] = JSON.parse(await fs.readText(lockPath));
  }
  return locks;
};

const actionSnapshot = (action: TargetAction): Record<string, unknown> => {
  if (action.kind === "install" || action.kind === "skip") {
    return {
      kind: action.kind,
      from: null,
      installed: null,
      will_write: action.kind === "install",
      blocked: false,
    };
  }

  if (action.kind === "update") {
    return {
      kind: action.kind,
      from: action.from,
      installed: null,
      will_write: true,
      blocked: false,
    };
  }

  if (action.kind === "downgrade") {
    return {
      kind: action.kind,
      from: action.from,
      installed: null,
      will_write: true,
      blocked: false,
    };
  }

  return {
    kind: action.kind,
    from: null,
    installed: action.installed,
    will_write: false,
    blocked: action.kind === "refuse-newer",
  };
};

const planTargetSnapshot = (target: TargetPlan): Record<string, unknown> => ({
  platform: target.platform,
  dest_dir: normalizedPathString(target.dest_dir),
  action: actionSnapshot(target.action),
  cmx_managed: target.cmx_managed,
});

const reportTargetSnapshot = (target: TargetOutcome): Record<string, unknown> => ({
  platform: target.platform,
  dest_dir: normalizedPathString(target.dest_dir),
  action: actionSnapshot(target.action),
  files_written: target.files_written,
  installed_checksum: target.installed_checksum,
  discarded_paths: target.discarded_paths.map(normalizedPathString),
});

export const snapshotPlan = (plan: InstallPlan): Record<string, unknown> => ({
  blocked: plan.targets.some((target) => actionSnapshot(target.action).blocked === true),
  cmx_present: plan.cmx_present,
  scope: plan.scope,
  source_checksum: plan.source_checksum,
  targets: plan.targets.map(planTargetSnapshot),
});

export const snapshotReport = (report: Report): Record<string, unknown> => ({
  tool_name: report.tool.name,
  scope: report.scope,
  source_registered: report.source_registered,
  targets: report.targets.map(reportTargetSnapshot),
});

const materializeTreeFile = async (
  fs: VirtualFilesystem,
  fixtureRelativePath: string,
  content: Uint8Array,
): Promise<void> => {
  const virtualPath = fixtureRelativePath.startsWith("home/")
    ? `/${fixtureRelativePath}`
    : fixtureRelativePath.startsWith("project/")
      ? `/${fixtureRelativePath}`
      : `/${fixtureRelativePath}`;
  const actualPath = fs.toActualPath(virtualPath);
  await mkdir(path.dirname(actualPath), { recursive: true });
  await writeFile(actualPath, content);
};

const readDirFiles = async (rootDir: string): Promise<Map<string, Uint8Array>> => {
  const files = new Map<string, Uint8Array>();
  await walkFixture(rootDir, rootDir, files);
  return files;
};

const walkFixture = async (
  rootDir: string,
  currentDir: string,
  output: Map<string, Uint8Array>,
): Promise<void> => {
  const entries = await readdir(currentDir, { withFileTypes: true }).catch(() => undefined);
  if (entries === undefined) {
    return;
  }

  for (const entry of entries) {
    const entryPath = path.join(currentDir, entry.name);
    if (entry.isDirectory()) {
      await walkFixture(rootDir, entryPath, output);
      continue;
    }

    output.set(path.relative(rootDir, entryPath).replaceAll("\\", "/"), await readFile(entryPath));
  }
};

export const materializeFixtureTree = async (
  fs: VirtualFilesystem,
  fixtureDir: string,
): Promise<void> => {
  const files = await readDirFiles(fixtureDir);
  for (const [relativePath, content] of files) {
    await materializeTreeFile(fs, relativePath, content);
  }
};

export const materializeFixtureLocks = async (
  fs: VirtualFilesystem,
  scope: InstallScope,
  fixtureDir: string,
): Promise<void> => {
  const files = await readDirFiles(fixtureDir);
  for (const [fileName, content] of files) {
    const virtualPath =
      scope === "global"
        ? `${VIRTUAL_CONFIG_ROOT}/${fileName}`
        : `${VIRTUAL_PROJECT_ROOT}/.context-mixer/${fileName}`;
    const actualPath = fs.toActualPath(virtualPath);
    await mkdir(path.dirname(actualPath), { recursive: true });
    await writeFile(actualPath, content);
  }
};

export const loadFixtureTree = async (fixtureDir: string): Promise<Record<string, string>> => {
  const entries = [...(await readDirFiles(fixtureDir)).entries()].map(
    ([relativePath, bytes]): [string, string] => [relativePath, decodeBytes(bytes)],
  );
  entries.sort((left, right) => left[0].localeCompare(right[0]));
  return Object.fromEntries(entries);
};

export const loadFixtureLocks = async (fixtureDir: string): Promise<Record<string, unknown>> => {
  const entries = [...(await readDirFiles(fixtureDir)).entries()].map(
    ([relativePath, bytes]): [string, unknown] => [
      relativePath,
      JSON.parse(decodeBytes(bytes)) as unknown,
    ],
  );
  entries.sort((left, right) => left[0].localeCompare(right[0]));
  return Object.fromEntries(entries);
};

export const loadFixtureJson = async <T>(fixturePath: string): Promise<T> =>
  JSON.parse(await readFile(fixturePath, "utf8")) as T;

export const loadBundledSkillFiles = async (
  fixtureDir: string,
): Promise<Array<{ relPath: string; content: string }>> =>
  [...(await readDirFiles(fixtureDir)).entries()]
    .map(([relativePath, bytes]) => ({
      relPath: relativePath,
      content: decodeBytes(bytes),
    }))
    .sort((left, right) => left.relPath.localeCompare(right.relPath));
