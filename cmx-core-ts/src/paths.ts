import os from "node:os";
import path from "node:path/posix";

import {
  agentExtension,
  lockFileName,
  type Platform,
  platformInstallSubpath,
  supportsArtifact,
} from "./platform.ts";
import { type ArtifactKind, type InstallScope, installedArtifactPath } from "./types.ts";

export class ConfigPaths {
  public readonly configDir: string;
  public readonly homeDir: string;
  public readonly platform: Platform;
  public readonly projectRoot: string;

  public constructor(args: {
    configDir: string;
    homeDir: string;
    platform: Platform;
    projectRoot: string;
  }) {
    this.configDir = args.configDir;
    this.homeDir = args.homeDir;
    this.platform = args.platform;
    this.projectRoot = args.projectRoot;
  }

  public static fromEnv(platform: Platform, projectRoot = process.cwd()): ConfigPaths {
    const homeDir = os.homedir().replaceAll("\\", "/");

    return new ConfigPaths({
      configDir: path.join(homeDir, ".config", "context-mixer"),
      homeDir,
      platform,
      projectRoot: projectRoot.replaceAll("\\", "/"),
    });
  }

  public withPlatform(platform: Platform): ConfigPaths {
    return new ConfigPaths({
      configDir: this.configDir,
      homeDir: this.homeDir,
      platform,
      projectRoot: this.projectRoot,
    });
  }

  public sourcesPath(): string {
    return path.join(this.configDir, "sources.json");
  }

  public gitClonesDir(): string {
    return path.join(this.configDir, "sources");
  }

  public configPath(): string {
    return path.join(this.configDir, "config.json");
  }

  public defaultArtifactHome(): string {
    return path.join(this.configDir, "home");
  }

  public setsPath(scope: InstallScope): string {
    return scope === "local"
      ? path.join(this.projectRoot, ".context-mixer", "sets.json")
      : path.join(this.configDir, "sets.json");
  }

  public lockPath(scope: InstallScope): string {
    const fileName = lockFileName(this.platform);

    return scope === "local"
      ? path.join(this.projectRoot, ".context-mixer", fileName)
      : path.join(this.configDir, fileName);
  }

  public installDir(kind: ArtifactKind, scope: InstallScope): string | null {
    const subpath = platformInstallSubpath(this.platform, kind, scope);
    if (subpath === null) {
      return null;
    }

    return scope === "local"
      ? path.join(this.projectRoot, subpath)
      : path.join(this.homeDir, subpath);
  }

  public installedArtifactPath(
    kind: ArtifactKind,
    name: string,
    scope: InstallScope,
  ): string | null {
    const installDir = this.installDir(kind, scope);
    if (installDir === null) {
      return null;
    }

    return installedArtifactPath(kind, name, installDir, agentExtension(this.platform));
  }

  public requireInstallDir(kind: ArtifactKind, scope: InstallScope): string {
    const installDir = this.installDir(kind, scope);
    if (installDir === null) {
      throw new Error(
        `The ${this.platform} platform does not support ${kind}s. ${this.platform} has no native ${kind} concept.`,
      );
    }

    return installDir;
  }

  public ensureSupports(kind: ArtifactKind): void {
    if (!supportsArtifact(this.platform, kind)) {
      throw new Error(
        `The ${this.platform} platform does not support ${kind}s. ${this.platform} has no native ${kind} concept.`,
      );
    }
  }
}
