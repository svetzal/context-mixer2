import { describe, expect, test } from "bun:test";

import { resolveLocalPath } from "../src/config.ts";
import { ConfigPaths } from "../src/paths.ts";
import { platformInstallSubpath } from "../src/platform.ts";
import {
  defaultCmxConfig,
  defaultLockFile,
  defaultSourcesFile,
  installedArtifactPath,
} from "../src/types.ts";

describe("ConfigPaths", () => {
  const paths = new ConfigPaths({
    configDir: "/home/testuser/.config/context-mixer",
    homeDir: "/home/testuser",
    platform: "copilot",
    projectRoot: "/project",
  });

  test("derives config file locations", () => {
    expect(paths.sourcesPath()).toBe("/home/testuser/.config/context-mixer/sources.json");
    expect(paths.gitClonesDir()).toBe("/home/testuser/.config/context-mixer/sources");
    expect(paths.configPath()).toBe("/home/testuser/.config/context-mixer/config.json");
    expect(paths.defaultArtifactHome()).toBe("/home/testuser/.config/context-mixer/home");
    expect(paths.setsPath("global")).toBe("/home/testuser/.config/context-mixer/sets.json");
    expect(paths.setsPath("local")).toBe("/project/.context-mixer/sets.json");
  });

  test("derives scope-aware lock and install paths", () => {
    expect(paths.lockPath("global")).toBe(
      "/home/testuser/.config/context-mixer/cmx-lock-copilot.json",
    );
    expect(paths.lockPath("local")).toBe("/project/.context-mixer/cmx-lock-copilot.json");
    expect(paths.installDir("skill", "global")).toBe("/home/testuser/.copilot/skills");
    expect(paths.installDir("skill", "local")).toBe("/project/.github/skills");
    expect(paths.installedArtifactPath("skill", "fixture-tool", "global")).toBe(
      "/home/testuser/.copilot/skills/fixture-tool",
    );
  });

  test("rejects unsupported artifact kinds", () => {
    const piPaths = paths.withPlatform("pi");
    expect(() => piPaths.requireInstallDir("agent", "global")).toThrow(
      "The pi platform does not support agents.",
    );
    expect(() => piPaths.ensureSupports("agent")).toThrow(
      "The pi platform does not support agents.",
    );
  });
});

describe("platform helpers", () => {
  test("covers scope-divergent skill paths", () => {
    expect(platformInstallSubpath("amp", "skill", "global")).toBe(".config/agents/skills");
    expect(platformInstallSubpath("amp", "skill", "local")).toBe(".agents/skills");
    expect(platformInstallSubpath("hermes", "skill", "global")).toBe(".hermes/skills");
    expect(platformInstallSubpath("copilot", "agent", "global")).toBe(".copilot/agents");
    expect(platformInstallSubpath("copilot", "agent", "local")).toBe(".github/agents");
    expect(platformInstallSubpath("windsurf", "agent", "global")).toBe(".codeium/windsurf/agents");
    expect(platformInstallSubpath("opencode", "agent", "local")).toBe(".opencode/agent");
    expect(platformInstallSubpath("codex", "agent", "global")).toBe(".codex/agents");
    expect(platformInstallSubpath("pi", "agent", "global")).toBeNull();
  });
});

describe("config and type defaults", () => {
  test("provide stable defaults", () => {
    expect(defaultCmxConfig()).toEqual({
      version: 1,
      llm: {
        gateway: "openai",
        model: "gpt-5.4",
      },
      external: [],
      platforms: [],
    });
    expect(defaultSourcesFile()).toEqual({ version: 1, sources: {} });
    expect(defaultLockFile()).toEqual({ version: 1, packages: {} });
  });

  test("resolves local and git source paths", () => {
    expect(resolveLocalPath({ type: "local", path: "/tmp/source" })).toBe("/tmp/source");
    expect(resolveLocalPath({ type: "git", local_clone: "/tmp/clone" })).toBe("/tmp/clone");
    expect(() => resolveLocalPath({ type: "local" })).toThrow(
      "Local source has no path configured",
    );
  });

  test("derives installed artifact leaf names", () => {
    expect(installedArtifactPath("agent", "reviewer", "/tmp/agents", "md")).toBe(
      "/tmp/agents/reviewer.md",
    );
    expect(installedArtifactPath("skill", "reviewer", "/tmp/skills", "md")).toBe(
      "/tmp/skills/reviewer",
    );
  });
});
