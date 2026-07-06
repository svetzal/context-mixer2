import { afterEach, describe, expect, test } from "bun:test";
import path from "node:path";

import {
  BundledSkill,
  canonicalFiles,
  checksumBundled,
  decideVersionGuardAction,
  lockFileName,
  platformInstallSubpath,
  reconcileSkillVersion,
  resolveTargets,
  SkillInstaller,
  saveConfig,
  saveLockFileTo,
  ToolIdentity,
  textSkillFile,
} from "../src/index.ts";
import type { Platform } from "../src/platform.ts";
import { defaultCmxConfig, type InstallScope } from "../src/types.ts";
import {
  FixedClock,
  loadBundledSkillFiles,
  loadFixtureJson,
  loadFixtureLocks,
  loadFixtureTree,
  makeTempFilesystem,
  materializeFixtureLocks,
  materializeFixtureTree,
  snapshotLocks,
  snapshotPlan,
  snapshotReport,
  snapshotTree,
} from "./helpers.ts";

const fixtureRoot = path.resolve(import.meta.dir, "../../cmx-core/conformance");
const cleanups: Array<() => Promise<void>> = [];
const checksumManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: { files: Array<{ path: string; content_utf8: string }> };
    expected: {
      sha256: string;
      canonical_order: string[];
      canonical_included_paths: string[];
    };
  }>;
}>(path.join(fixtureRoot, "checksum/manifest.json"));
const frontmatterManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: { version: string; skill_md_path: string };
    expected: { skill_md_path: string; idempotent_second_pass: boolean };
  }>;
}>(path.join(fixtureRoot, "frontmatter/manifest.json"));
const versionGuardManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: {
      bundled_version: string;
      tracked: boolean;
      installed_version: string | null;
      disk_state: "missing" | "matches-source" | "drifted";
      force: boolean;
    };
    expected: {
      kind: string;
      from: string | null;
      installed: string | null;
      will_write: boolean;
      blocked: boolean;
    };
  }>;
}>(path.join(fixtureRoot, "version-guard/manifest.json"));
const pathsManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: { platform: Platform; kind: "skill"; scope: InstallScope };
    expected: { subpath: string; lockname: string };
  }>;
}>(path.join(fixtureRoot, "paths/manifest.json"));
const targetResolveManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: {
      scope: InstallScope;
      config_platforms: string[];
      non_empty_locks: string[];
    };
    expected: { resolved_platforms: string[] };
  }>;
}>(path.join(fixtureRoot, "target-resolve/manifest.json"));
const installE2eManifest = await loadFixtureJson<{
  cases: Array<{
    name: string;
    input: {
      tool_name: string;
      tool_version: string;
      scope: InstallScope;
      force: boolean;
      bundle_dir: string;
      pre_tree_dir: string;
      pre_locks_dir: string;
    };
    expected: {
      tree_dir: string;
      locks_dir: string;
      report_path: string;
    };
  }>;
}>(path.join(fixtureRoot, "install-e2e/manifest.json"));

const onlyFile = <T>(value: T | undefined, message: string): T => {
  if (value === undefined) {
    throw new Error(message);
  }

  return value;
};

afterEach(async () => {
  while (cleanups.length > 0) {
    const cleanup = cleanups.pop();
    if (cleanup !== undefined) {
      await cleanup();
    }
  }
});

describe("checksum fixtures", () => {
  for (const fixture of checksumManifest.cases) {
    test(fixture.name, () => {
      const files = fixture.input.files.map((file) => textSkillFile(file.path, file.content_utf8));
      expect(checksumBundled(files)).toBe(fixture.expected.sha256);
      expect(canonicalFiles(files).map((file) => file.relPath)).toEqual(
        fixture.expected.canonical_order,
      );
      expect(canonicalFiles(files).map((file) => file.relPath)).toEqual(
        fixture.expected.canonical_included_paths,
      );
    });
  }
});

describe("frontmatter fixtures", () => {
  for (const fixture of frontmatterManifest.cases) {
    test(fixture.name, async () => {
      const inputPath = path.join(fixtureRoot, "frontmatter", fixture.input.skill_md_path);
      const expectedPath = path.join(fixtureRoot, "frontmatter", fixture.expected.skill_md_path);
      const input = await Bun.file(inputPath).text();
      const expected = await Bun.file(expectedPath).text();
      const once = onlyFile(
        reconcileSkillVersion([textSkillFile("SKILL.md", input)], fixture.input.version)[0],
        "expected reconciled SKILL.md output",
      );
      expect(new TextDecoder().decode(once.bytes)).toBe(expected);

      if (fixture.expected.idempotent_second_pass) {
        const twice = onlyFile(
          reconcileSkillVersion([once], fixture.input.version)[0],
          "expected idempotent SKILL.md output",
        );
        expect(new TextDecoder().decode(twice.bytes)).toBe(expected);
      }
    });
  }
});

describe("version guard fixtures", () => {
  for (const fixture of versionGuardManifest.cases) {
    test(fixture.name, () => {
      const action = decideVersionGuardAction({
        bundledVersion: fixture.input.bundled_version,
        tracked: fixture.input.tracked,
        installedVersion: fixture.input.installed_version,
        diskState: fixture.input.disk_state,
        force: fixture.input.force,
      });
      const snapshot =
        action.kind === "install" || action.kind === "skip"
          ? {
              kind: action.kind,
              from: null,
              installed: null,
              will_write: action.kind === "install",
              blocked: false,
            }
          : action.kind === "update" || action.kind === "downgrade"
            ? {
                kind: action.kind,
                from: action.from,
                installed: null,
                will_write: true,
                blocked: false,
              }
            : {
                kind: action.kind,
                from: null,
                installed: action.installed,
                will_write: false,
                blocked: action.kind === "refuse-newer",
              };

      expect(snapshot as Record<string, unknown>).toEqual({
        kind: fixture.expected.kind,
        from: fixture.expected.from,
        installed: fixture.expected.installed,
        will_write: fixture.expected.will_write,
        blocked: fixture.expected.blocked,
      });
    });
  }
});

describe("paths fixtures", () => {
  for (const fixture of pathsManifest.cases) {
    test(fixture.name, () => {
      expect(
        platformInstallSubpath(fixture.input.platform, fixture.input.kind, fixture.input.scope),
      ).toBe(fixture.expected.subpath);
      expect(lockFileName(fixture.input.platform)).toBe(fixture.expected.lockname);
    });
  }
});

describe("target resolve fixtures", () => {
  for (const fixture of targetResolveManifest.cases) {
    test(fixture.name, async () => {
      const { fs, paths, cleanup } = await makeTempFilesystem();
      cleanups.push(cleanup);

      if (fixture.input.config_platforms.length > 0) {
        await saveConfig(
          {
            ...defaultCmxConfig(),
            platforms: fixture.input.config_platforms,
          },
          fs,
          paths,
        );
      }

      for (const platform of fixture.input.non_empty_locks) {
        const lockPath = paths.withPlatform(platform as Platform).lockPath(fixture.input.scope);
        await saveLockFileTo(
          {
            version: 1,
            packages: {
              fixture: {
                type: "skill",
                version: "1.0.0",
                installed_at: "2026-07-05T12:00:00+00:00",
                source: { repo: "bundled:fixture", path: "skills/fixture" },
                source_checksum: "sha256:test",
                installed_checksum: "sha256:test",
              },
            },
          },
          lockPath,
          fs,
        );
      }

      const resolved = await resolveTargets(undefined, "skill", fixture.input.scope, { fs, paths });
      expect([...resolved] as string[]).toEqual([...fixture.expected.resolved_platforms]);
    });
  }
});

describe("install e2e fixtures", () => {
  for (const fixture of installE2eManifest.cases) {
    test(fixture.name, async () => {
      const { fs, paths, cleanup } = await makeTempFilesystem();
      cleanups.push(cleanup);

      const bundleDir = path.join(fixtureRoot, "install-e2e", fixture.input.bundle_dir);
      const preTreeDir = path.join(fixtureRoot, "install-e2e", fixture.input.pre_tree_dir);
      const preLocksDir = path.join(fixtureRoot, "install-e2e", fixture.input.pre_locks_dir);
      const expectedTreeDir = path.join(fixtureRoot, "install-e2e", fixture.expected.tree_dir);
      const expectedLocksDir = path.join(fixtureRoot, "install-e2e", fixture.expected.locks_dir);
      const expectedReportPath = path.join(
        fixtureRoot,
        "install-e2e",
        fixture.expected.report_path,
      );

      await materializeFixtureTree(fs, preTreeDir);
      await materializeFixtureLocks(fs, fixture.input.scope, preLocksDir);

      const bundleFiles = await loadBundledSkillFiles(bundleDir);
      const skill = BundledSkill.fromFiles(
        bundleFiles.map((file) => textSkillFile(file.relPath, file.content)),
      );
      const installer = new SkillInstaller(
        new ToolIdentity(fixture.input.tool_name, fixture.input.tool_version),
      );
      const context = {
        fs,
        clock: new FixedClock(),
        paths,
      };

      const plan = await installer.plan(skill, fixture.input.scope, fixture.input.force, context);
      const planSnapshot = snapshotPlan(plan);

      let applySnapshot: Record<string, unknown>;
      try {
        const report = await installer.apply(skill, plan, context);
        applySnapshot = {
          status: "applied",
          error: null,
          report: snapshotReport(report),
        };
      } catch (error) {
        applySnapshot = {
          status: "blocked",
          error: error instanceof Error ? error.message : String(error),
          report: null,
        };
      }

      expect(await snapshotTree(fs, paths, fixture.input.scope)).toEqual(
        await loadFixtureTree(expectedTreeDir),
      );
      expect(await snapshotLocks(fs, paths, fixture.input.scope)).toEqual(
        await loadFixtureLocks(expectedLocksDir),
      );
      expect({ plan: planSnapshot, apply: applySnapshot }).toEqual(
        await loadFixtureJson(expectedReportPath),
      );
    });
  }
});
