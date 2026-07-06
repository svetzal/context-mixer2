import path from "node:path/posix";

import { coerce, compare as compareSemver } from "semver";
import { checksumDir } from "./checksum.ts";
import {
  loadConfig,
  loadSources,
  managedOrAllPlatforms,
  managedPlatforms,
  mutateSources,
  resolveArtifactHome,
} from "./config.ts";
import { reconcileSkillVersion } from "./frontmatter.ts";
import type { Clock, Filesystem } from "./gateway.ts";
import { loadLockFile, mutateLockFile } from "./lockfile.ts";
import type { ConfigPaths } from "./paths.ts";
import { PLATFORM_VALUES, type Platform } from "./platform.ts";
import {
  canonicalFiles,
  checksumBundled,
  type SkillFile,
  textSkillFile,
  writeSkillFiles,
} from "./skill-fs.ts";
import { resolveTargets } from "./targets.ts";
import type { InstallScope, LockEntry, SourceEntry } from "./types.ts";

export type Scope = InstallScope;

export class ToolIdentity {
  public readonly name: string;
  public readonly version: string;

  public constructor(name: string, version: string) {
    this.name = name;
    this.version = version;
  }
}

export class BundledSkill {
  public readonly files: SkillFile[];

  public constructor(files: SkillFile[]) {
    this.files = files;
  }

  public static fromFiles(files: SkillFile[]): BundledSkill {
    return new BundledSkill(files);
  }

  public static singleMd(content: string): BundledSkill {
    return new BundledSkill([textSkillFile("SKILL.md", content)]);
  }

  public hasSkillMd(): boolean {
    return this.files.some((file) => file.relPath === "SKILL.md");
  }
}

export type DiskState = "missing" | "matches-source" | "drifted";

export type TargetAction =
  | { kind: "install" }
  | { kind: "update"; from: string | null }
  | { kind: "skip" }
  | { kind: "drifted-skip"; installed: string }
  | { kind: "refuse-newer"; installed: string }
  | { kind: "downgrade"; from: string };

export interface PlannedFile {
  rel_path: string;
  dest_path: string;
}

export interface TargetPlan {
  platform: Platform;
  scope: InstallScope;
  dest_dir: string;
  files: PlannedFile[];
  action: TargetAction;
  cmx_managed: boolean;
}

export interface InstallPlan {
  tool: ToolIdentity;
  scope: InstallScope;
  source_checksum: string;
  cmx_present: boolean;
  force: boolean;
  targets: TargetPlan[];
}

export interface TargetOutcome {
  platform: Platform;
  dest_dir: string;
  action: TargetAction;
  files_written: number;
  installed_checksum: string | null;
  discarded_paths: string[];
}

export interface Report {
  tool: ToolIdentity;
  scope: InstallScope;
  targets: TargetOutcome[];
  source_registered: boolean;
}

export interface TargetStatus {
  platform: Platform;
  installed: boolean;
  installed_version: string | null;
  drifted: boolean;
  tracked: boolean;
}

export interface Status {
  tool_name: string;
  scope: InstallScope;
  targets: TargetStatus[];
}

export interface RemoveReport {
  tool_name: string;
  scope: InstallScope;
  removed_dirs: string[];
  platforms_cleared: Platform[];
  source_unregistered: boolean;
  was_on_disk: boolean;
  was_tracked: boolean;
}

export interface InstallerContext {
  fs: Filesystem;
  clock: Clock;
  paths: ConfigPaths;
}

const willWrite = (action: TargetAction): boolean =>
  action.kind === "install" || action.kind === "update" || action.kind === "downgrade";

const isBlocked = (action: TargetAction): boolean => action.kind === "refuse-newer";

const formatTimestamp = (date: Date): string => {
  const pad = (value: number): string => `${value}`.padStart(2, "0");

  return (
    `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}` +
    `T${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}+00:00`
  );
};

const compareVersions = (installedVersion: string | null, bundledVersion: string): number => {
  if (installedVersion === null) {
    return -1;
  }

  const installedSemver = coerce(installedVersion);
  const bundledSemver = coerce(bundledVersion);
  if (installedSemver !== null && bundledSemver !== null) {
    return compareSemver(installedSemver, bundledSemver);
  }

  return installedVersion === bundledVersion ? 0 : -1;
};

export interface VersionGuardDecisionInput {
  bundledVersion: string;
  tracked: boolean;
  installedVersion: string | null;
  diskState: DiskState;
  force: boolean;
}

export const decideVersionGuardAction = (input: VersionGuardDecisionInput): TargetAction => {
  if (!input.tracked) {
    return { kind: "install" };
  }

  const comparison = compareVersions(input.installedVersion, input.bundledVersion);
  if (comparison < 0) {
    return { kind: "update", from: input.installedVersion };
  }

  if (comparison === 0) {
    if (input.diskState === "missing") {
      return { kind: "install" };
    }

    if (input.diskState === "matches-source") {
      return { kind: "skip" };
    }

    return input.force
      ? { kind: "update", from: input.installedVersion }
      : {
          kind: "drifted-skip",
          installed: input.installedVersion ?? "unknown",
        };
  }

  return input.force
    ? { kind: "downgrade", from: input.installedVersion ?? "unknown" }
    : { kind: "refuse-newer", installed: input.installedVersion ?? "unknown" };
};

const buildLockEntry = (tool: ToolIdentity, checksum: string, installedAt: string): LockEntry => ({
  type: "skill",
  version: tool.version,
  installed_at: installedAt,
  source: {
    repo: `bundled:${tool.name}`,
    path: `skills/${tool.name}`,
  },
  source_checksum: checksum,
  installed_checksum: checksum,
});

const computeDiskState = async (
  skillDest: string,
  sourceChecksum: string,
  fs: Filesystem,
): Promise<DiskState> => {
  if (!(await fs.exists(skillDest))) {
    return "missing";
  }

  const diskChecksum = await checksumDir(skillDest, fs);
  return diskChecksum === sourceChecksum ? "matches-source" : "drifted";
};

const unique = <T>(values: Iterable<T>): T[] => [...new Set(values)];

const discardedPathsAgainstBundle = async (
  skillDest: string,
  bundledFiles: readonly SkillFile[],
  fs: Filesystem,
): Promise<string[]> => {
  if (!(await fs.exists(skillDest))) {
    return [];
  }

  const installedFiles = await collectFilesRecursive(skillDest, skillDest, fs);
  const installedByRel = new Map(installedFiles.map((file) => [file.relPath, file.bytes]));
  const bundledByRel = new Map(
    canonicalFiles(bundledFiles).map((file) => [file.relPath, file.bytes]),
  );
  const relativePaths = unique([...installedByRel.keys(), ...bundledByRel.keys()]).sort(
    (left, right) => left.localeCompare(right),
  );

  return relativePaths.flatMap((relPath) => {
    const installed = installedByRel.get(relPath);
    const bundled = bundledByRel.get(relPath);
    const equal =
      installed !== undefined &&
      bundled !== undefined &&
      installed.length === bundled.length &&
      installed.every((byte, index) => byte === bundled[index]);

    return equal ? [] : [path.join(skillDest, relPath)];
  });
};

const collectFilesRecursive = async (
  rootDir: string,
  currentDir: string,
  fs: Filesystem,
): Promise<SkillFile[]> => {
  if (!(await fs.exists(currentDir))) {
    return [];
  }

  const entries = await fs.listDir(currentDir);
  const files: SkillFile[] = [];

  for (const entry of entries) {
    const absolutePath = path.join(currentDir, entry.fileName);
    if (entry.isDirectory) {
      files.push(...(await collectFilesRecursive(rootDir, absolutePath, fs)));
      continue;
    }

    files.push({
      relPath: path.relative(rootDir, absolutePath),
      bytes: await fs.read(absolutePath),
    });
  }

  return files;
};

export class SkillInstaller {
  public readonly tool: ToolIdentity;

  public constructor(tool: ToolIdentity) {
    this.tool = tool;
  }

  public async plan(
    skill: BundledSkill,
    scope: Scope,
    force: boolean,
    context: InstallerContext,
  ): Promise<InstallPlan> {
    if (!skill.hasSkillMd()) {
      throw new Error(`BundledSkill for '${this.tool.name}' is missing SKILL.md`);
    }

    const files = reconcileSkillVersion(skill.files, this.tool.version);
    const sourceChecksum = checksumBundled(files);
    const targets = await resolveTargets(undefined, "skill", scope, context);
    const managed = await managedPlatforms(context.fs, context.paths);
    const cmxManaged = managed !== undefined;
    const cmxPresent =
      cmxManaged ||
      (
        await Promise.all(
          PLATFORM_VALUES.map(async (platform) => {
            const lockFile = await loadLockFile(
              scope,
              context.fs,
              context.paths.withPlatform(platform),
            );
            return Object.keys(lockFile.packages).length > 0;
          }),
        )
      ).some(Boolean);

    const targetPlans: TargetPlan[] = [];
    for (const platform of targets) {
      const platformPaths = context.paths.withPlatform(platform);
      const skillDest = path.join(platformPaths.requireInstallDir("skill", scope), this.tool.name);
      const plannedFiles = files.map((file) => ({
        rel_path: file.relPath,
        dest_path: path.join(skillDest, file.relPath),
      }));
      const lockFile = await loadLockFile(scope, context.fs, platformPaths);
      const entry = lockFile.packages[this.tool.name];
      const action =
        entry === undefined
          ? { kind: "install" as const }
          : decideVersionGuardAction({
              bundledVersion: this.tool.version,
              tracked: true,
              installedVersion: entry.version ?? null,
              diskState: await computeDiskState(skillDest, sourceChecksum, context.fs),
              force,
            });

      targetPlans.push({
        platform,
        scope,
        dest_dir: skillDest,
        files: plannedFiles,
        action,
        cmx_managed: cmxManaged,
      });
    }

    return {
      tool: this.tool,
      scope,
      source_checksum: sourceChecksum,
      cmx_present: cmxPresent,
      force,
      targets: targetPlans,
    };
  }

  public async apply(
    skill: BundledSkill,
    plan: InstallPlan,
    context: InstallerContext,
  ): Promise<Report> {
    if (plan.targets.some((target) => isBlocked(target.action))) {
      throw new Error(
        `Install plan for '${this.tool.name}' is blocked. Run with force=true to override.`,
      );
    }

    const files = reconcileSkillVersion(skill.files, this.tool.version);
    const currentChecksum = checksumBundled(files);
    if (currentChecksum !== plan.source_checksum) {
      throw new Error(
        `Parity check failed for '${this.tool.name}': the BundledSkill has changed since plan() was called.`,
      );
    }

    const dirsToWrite = unique(
      plan.targets.filter((target) => willWrite(target.action)).map((target) => target.dest_dir),
    );
    const replaceDirs = unique(
      plan.targets
        .filter(
          (target) =>
            plan.force && (target.action.kind === "update" || target.action.kind === "downgrade"),
        )
        .map((target) => target.dest_dir),
    );
    const discardedByDir = new Map<string, string[]>();

    for (const dir of replaceDirs) {
      discardedByDir.set(dir, await discardedPathsAgainstBundle(dir, files, context.fs));
      if (await context.fs.exists(dir)) {
        await context.fs.removeDirAll(dir);
      }
    }

    for (const dir of dirsToWrite) {
      await writeSkillFiles(dir, files, context.fs);
    }

    const installedAt = formatTimestamp(context.clock.now());
    const installedChecksum = plan.source_checksum;
    const targets: TargetOutcome[] = [];

    for (const target of plan.targets) {
      if (!willWrite(target.action)) {
        targets.push({
          platform: target.platform,
          dest_dir: target.dest_dir,
          action: target.action,
          files_written: 0,
          installed_checksum: null,
          discarded_paths: [],
        });
        continue;
      }

      await mutateLockFile(
        scopeFromPlan(target.scope),
        context.fs,
        context.paths.withPlatform(target.platform),
        (lockFile) => {
          lockFile.packages[this.tool.name] = buildLockEntry(
            this.tool,
            installedChecksum,
            installedAt,
          );
        },
      );

      targets.push({
        platform: target.platform,
        dest_dir: target.dest_dir,
        action: target.action,
        files_written: target.files.length,
        installed_checksum: installedChecksum,
        discarded_paths: discardedByDir.get(target.dest_dir) ?? [],
      });
    }

    const sourceRegistered =
      (await managedPlatforms(context.fs, context.paths)) !== undefined
        ? await this.registerManagedSource(files, context)
        : false;

    return {
      tool: this.tool,
      scope: plan.scope,
      targets,
      source_registered: sourceRegistered,
    };
  }

  public async status(scope: Scope, context: InstallerContext): Promise<Status> {
    const targets = await resolveTargets(undefined, "skill", scope, context);
    const statuses: TargetStatus[] = [];

    for (const platform of targets) {
      const platformPaths = context.paths.withPlatform(platform);
      const skillDir = path.join(platformPaths.requireInstallDir("skill", scope), this.tool.name);
      const installed = await context.fs.exists(skillDir);
      const lockFile = await loadLockFile(scope, context.fs, platformPaths);
      const entry = lockFile.packages[this.tool.name];
      const tracked = entry !== undefined;
      const drifted =
        installed && tracked
          ? (await checksumDir(skillDir, context.fs)) !== entry.installed_checksum
          : false;

      statuses.push({
        platform,
        installed,
        installed_version: entry?.version ?? null,
        drifted,
        tracked,
      });
    }

    return {
      tool_name: this.tool.name,
      scope,
      targets: statuses,
    };
  }

  public async remove(scope: Scope, context: InstallerContext): Promise<RemoveReport> {
    const platforms = (await managedOrAllPlatforms(context.fs, context.paths)).filter(
      (platform) => platform !== undefined,
    ) as Platform[];
    const skillPlatforms = platforms;
    const dirsToDelete = new Set<string>();
    const platformsCleared: Platform[] = [];
    let wasTracked = false;

    for (const platform of skillPlatforms) {
      const platformPaths = context.paths.withPlatform(platform);
      const skillDir = path.join(platformPaths.requireInstallDir("skill", scope), this.tool.name);
      if (await context.fs.exists(skillDir)) {
        dirsToDelete.add(skillDir);
      }

      const lockFile = await loadLockFile(scope, context.fs, platformPaths);
      if (lockFile.packages[this.tool.name] !== undefined) {
        await mutateLockFile(scope, context.fs, platformPaths, (mutableLock) => {
          delete mutableLock.packages[this.tool.name];
        });
        platformsCleared.push(platform);
        wasTracked = true;
      }
    }

    const removedDirs = [...dirsToDelete];
    for (const dir of removedDirs) {
      await context.fs.removeDirAll(dir);
    }

    const sourceName = `bundled:${this.tool.name}`;
    let sourceUnregistered = false;
    try {
      const sources = await loadSources(context.fs, context.paths);
      const entry = sources.sources[sourceName];
      if (entry !== undefined) {
        if (entry.path !== undefined && (await context.fs.exists(entry.path))) {
          await context.fs.removeDirAll(entry.path);
        }
        await mutateSources(context.fs, context.paths, (mutableSources) => {
          delete mutableSources.sources[sourceName];
        });
        sourceUnregistered = true;
      }
    } catch {
      sourceUnregistered = false;
    }

    return {
      tool_name: this.tool.name,
      scope,
      removed_dirs: removedDirs,
      platforms_cleared: platformsCleared,
      source_unregistered: sourceUnregistered,
      was_on_disk: removedDirs.length > 0,
      was_tracked: wasTracked,
    };
  }

  private async registerManagedSource(
    files: SkillFile[],
    context: InstallerContext,
  ): Promise<boolean> {
    const config = await loadConfig(context.fs, context.paths);
    const materializedDir = path.join(
      resolveArtifactHome(config, context.paths),
      "skills",
      this.tool.name,
    );
    await writeSkillFiles(materializedDir, files, context.fs);

    const sourceName = `bundled:${this.tool.name}`;
    const now = formatTimestamp(context.clock.now());
    await mutateSources(context.fs, context.paths, (sources) => {
      if (sources.sources[sourceName] === undefined) {
        const entry: SourceEntry = {
          type: "local",
          path: materializedDir,
          last_updated: now,
        };
        sources.sources[sourceName] = entry;
      }
    });
    return true;
  }
}

const scopeFromPlan = (scope: InstallScope): InstallScope => scope;
