import path from "node:path/posix";

import { markdownToCodexToml } from "./agent.ts";
import { checksumBytes } from "./checksum.ts";
import { loadConfig, managedPlatforms, mutateSources, resolveArtifactHome } from "./config.ts";
import { reconcileDocumentVersion } from "./frontmatter.ts";
import { loadLockFile, mutateLockFile } from "./lockfile.ts";
import type { Platform } from "./platform.ts";
import {
  BundledSkill,
  decideVersionGuardAction,
  type InstallerContext,
  type InstallPlan,
  type Report,
  SkillInstaller,
  type TargetAction,
  ToolIdentity,
} from "./skill-installer.ts";
import { resolveTargets } from "./targets.ts";
import type { ArtifactKind, InstallScope, LockEntry, SourceEntry } from "./types.ts";

const encoder = new TextEncoder();

export class ArtifactIdentity {
  public constructor(
    public readonly name: string,
    public readonly version: string,
  ) {}
}

export class BundledArtifact {
  private constructor(
    public readonly kind: ArtifactKind,
    public readonly agentMarkdown?: string,
    public readonly skill?: BundledSkill,
  ) {}

  public static agent(markdown: string): BundledArtifact {
    return new BundledArtifact("agent", markdown);
  }

  public static skillMd(markdown: string): BundledArtifact {
    return new BundledArtifact("skill", undefined, BundledSkill.singleMd(markdown));
  }

  public static skillBundle(skill: BundledSkill): BundledArtifact {
    return new BundledArtifact("skill", undefined, skill);
  }
}

export interface AgentTargetPlan {
  platform: Platform;
  dest_path: string;
  action: TargetAction;
  installed_bytes: Uint8Array;
  installed_checksum: string;
}

export interface AgentInstallPlan {
  artifact: ArtifactIdentity;
  scope: InstallScope;
  source_checksum: string;
  targets: AgentTargetPlan[];
  force: boolean;
  cmx_managed: boolean;
}

export type ArtifactInstallPlan =
  | { kind: "agent"; plan: AgentInstallPlan }
  | { kind: "skill"; plan: InstallPlan };

export interface AgentTargetOutcome {
  platform: Platform;
  dest_path: string;
  action: TargetAction;
}

export interface AgentInstallReport {
  artifact: ArtifactIdentity;
  targets: AgentTargetOutcome[];
  source_registered: boolean;
}

export type ArtifactInstallReport =
  | { kind: "agent"; report: AgentInstallReport }
  | { kind: "skill"; report: Report };

const willWrite = (action: TargetAction): boolean =>
  action.kind === "install" || action.kind === "update" || action.kind === "downgrade";

const timestamp = (date: Date): string => date.toISOString().replace(".000Z", "+00:00");

export class ArtifactInstaller {
  public constructor(public readonly artifact: ArtifactIdentity) {}

  public async plan(
    bundle: BundledArtifact,
    scope: InstallScope,
    force: boolean,
    context: InstallerContext,
  ): Promise<ArtifactInstallPlan> {
    if (bundle.kind === "skill") {
      if (bundle.skill === undefined) throw new Error("skill bundle is missing");
      const installer = new SkillInstaller(
        new ToolIdentity(this.artifact.name, this.artifact.version),
      );
      return { kind: "skill", plan: await installer.plan(bundle.skill, scope, force, context) };
    }
    if (bundle.agentMarkdown === undefined) throw new Error("agent markdown is missing");
    const source = reconcileDocumentVersion(bundle.agentMarkdown, this.artifact.version);
    const sourceChecksum = checksumBytes(encoder.encode(source));
    const platforms = await resolveTargets(undefined, "agent", scope, context);
    const cmxManaged = (await managedPlatforms(context.fs, context.paths)) !== undefined;
    const targets: AgentTargetPlan[] = [];

    for (const platform of platforms) {
      const platformPaths = context.paths.withPlatform(platform);
      const destPath = platformPaths.installedArtifactPath("agent", this.artifact.name, scope);
      if (destPath === null) throw new Error(`The ${platform} platform does not support agents.`);
      const installedBytes = encoder.encode(
        platform === "codex" ? markdownToCodexToml(source, this.artifact.name) : source,
      );
      const installedChecksum = checksumBytes(installedBytes);
      const lock = await loadLockFile(scope, context.fs, platformPaths);
      const entry = lock.packages[this.artifact.name];
      let diskState: "missing" | "matches-source" | "drifted" = "missing";
      if (await context.fs.exists(destPath)) {
        diskState =
          checksumBytes(await context.fs.read(destPath)) === installedChecksum
            ? "matches-source"
            : "drifted";
      }
      const action = decideVersionGuardAction({
        bundledVersion: this.artifact.version,
        tracked: entry !== undefined,
        installedVersion: entry?.version ?? null,
        diskState,
        force,
      });
      targets.push({
        platform,
        dest_path: destPath,
        action,
        installed_bytes: installedBytes,
        installed_checksum: installedChecksum,
      });
    }
    return {
      kind: "agent",
      plan: {
        artifact: this.artifact,
        scope,
        source_checksum: sourceChecksum,
        targets,
        force,
        cmx_managed: cmxManaged,
      },
    };
  }

  public async apply(
    bundle: BundledArtifact,
    installPlan: ArtifactInstallPlan,
    context: InstallerContext,
  ): Promise<ArtifactInstallReport> {
    if (bundle.kind === "skill" && installPlan.kind === "skill") {
      if (bundle.skill === undefined) throw new Error("skill bundle is missing");
      const installer = new SkillInstaller(
        new ToolIdentity(this.artifact.name, this.artifact.version),
      );
      return {
        kind: "skill",
        report: await installer.apply(bundle.skill, installPlan.plan, context),
      };
    }
    if (
      bundle.kind !== "agent" ||
      installPlan.kind !== "agent" ||
      bundle.agentMarkdown === undefined
    ) {
      throw new Error("artifact kind does not match install plan");
    }
    const plan = installPlan.plan;
    if (plan.targets.some((target) => target.action.kind === "refuse-newer")) {
      throw new Error(
        `Install plan for '${this.artifact.name}' is blocked. Run with force=true to override.`,
      );
    }
    const source = reconcileDocumentVersion(bundle.agentMarkdown, this.artifact.version);
    if (checksumBytes(encoder.encode(source)) !== plan.source_checksum) {
      throw new Error(
        `Parity check failed for '${this.artifact.name}': the BundledArtifact has changed since plan() was called.`,
      );
    }
    const installedAt = timestamp(context.clock.now());
    for (const target of plan.targets) {
      if (!willWrite(target.action)) continue;
      await context.fs.createDirAll(path.dirname(target.dest_path));
      await context.fs.writeBytes(target.dest_path, target.installed_bytes);
      await mutateLockFile(
        plan.scope,
        context.fs,
        context.paths.withPlatform(target.platform),
        (lock) => {
          const entry: LockEntry = {
            type: "agent",
            version: this.artifact.version,
            installed_at: installedAt,
            source: {
              repo: `bundled:${this.artifact.name}`,
              path: `agents/${this.artifact.name}.md`,
            },
            source_checksum: plan.source_checksum,
            installed_checksum: target.installed_checksum,
          };
          lock.packages[this.artifact.name] = entry;
        },
      );
    }
    let sourceRegistered = false;
    if (plan.cmx_managed) {
      const config = await loadConfig(context.fs, context.paths);
      const materialized = path.join(
        resolveArtifactHome(config, context.paths),
        "agents",
        `${this.artifact.name}.md`,
      );
      await context.fs.createDirAll(path.dirname(materialized));
      await context.fs.write(materialized, source);
      await mutateSources(context.fs, context.paths, (sources) => {
        const entry: SourceEntry = { type: "local", path: materialized, last_updated: installedAt };
        sources.sources[`bundled:${this.artifact.name}`] = entry;
      });
      sourceRegistered = true;
    }
    return {
      kind: "agent",
      report: {
        artifact: this.artifact,
        targets: plan.targets.map((target) => ({
          platform: target.platform,
          dest_path: target.dest_path,
          action: target.action,
        })),
        source_registered: sourceRegistered,
      },
    };
  }
}
