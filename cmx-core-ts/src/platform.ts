import type { ArtifactKind, InstallScope } from "./types.ts";

export const PLATFORM_VALUES = [
  "claude",
  "copilot",
  "cursor",
  "windsurf",
  "gemini",
  "opencode",
  "codex",
  "pi",
  "crush",
  "amp",
  "zed",
  "openhands",
  "hermes",
  "devin",
] as const;

export type Platform = (typeof PLATFORM_VALUES)[number];

interface PlatformSpec {
  slug: string;
  skillSubpath: {
    global: string;
    local: string;
  };
  supportsAgent: boolean;
  agentExtension: "md" | "toml";
}

const sharedAgentsSkills = ".agents/skills";

const PLATFORM_SPECS = {
  claude: {
    slug: "",
    skillSubpath: { global: ".claude/skills", local: ".claude/skills" },
    supportsAgent: true,
    agentExtension: "md",
  },
  copilot: {
    slug: "copilot",
    skillSubpath: { global: ".copilot/skills", local: ".github/skills" },
    supportsAgent: true,
    agentExtension: "md",
  },
  cursor: {
    slug: "cursor",
    skillSubpath: { global: ".cursor/skills", local: ".cursor/skills" },
    supportsAgent: true,
    agentExtension: "md",
  },
  windsurf: {
    slug: "windsurf",
    skillSubpath: {
      global: ".codeium/windsurf/skills",
      local: ".windsurf/skills",
    },
    supportsAgent: true,
    agentExtension: "md",
  },
  gemini: {
    slug: "gemini",
    skillSubpath: { global: ".gemini/skills", local: ".gemini/skills" },
    supportsAgent: true,
    agentExtension: "md",
  },
  opencode: {
    slug: "opencode",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: true,
    agentExtension: "md",
  },
  codex: {
    slug: "codex",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: true,
    agentExtension: "toml",
  },
  pi: {
    slug: "pi",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  crush: {
    slug: "crush",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  amp: {
    slug: "amp",
    skillSubpath: { global: ".config/agents/skills", local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  zed: {
    slug: "zed",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  openhands: {
    slug: "openhands",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  hermes: {
    slug: "hermes",
    skillSubpath: { global: ".hermes/skills", local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
  devin: {
    slug: "devin",
    skillSubpath: { global: sharedAgentsSkills, local: sharedAgentsSkills },
    supportsAgent: false,
    agentExtension: "md",
  },
} as const satisfies Record<Platform, PlatformSpec>;

export const isPlatform = (value: string): value is Platform =>
  PLATFORM_VALUES.includes(value as Platform);

export const platformSlug = (platform: Platform): string => PLATFORM_SPECS[platform].slug;

export const platformInstallSubpath = (
  platform: Platform,
  kind: ArtifactKind,
  scope: InstallScope,
): string | null => {
  if (kind === "skill") {
    return PLATFORM_SPECS[platform].skillSubpath[scope];
  }

  if (!PLATFORM_SPECS[platform].supportsAgent) {
    return null;
  }

  if (platform === "copilot") {
    const base = scope === "local" ? ".github" : ".copilot";
    return `${base}/agents`;
  }

  if (platform === "windsurf") {
    return scope === "local" ? ".windsurf/agents" : ".codeium/windsurf/agents";
  }

  if (platform === "opencode") {
    return scope === "local" ? ".opencode/agent" : ".config/opencode/agent";
  }

  if (platform === "codex") {
    return ".codex/agents";
  }

  return `.${platform}/agents`;
};

export const supportsArtifact = (platform: Platform, kind: ArtifactKind): boolean =>
  kind === "skill" || PLATFORM_SPECS[platform].supportsAgent;

export const agentExtension = (platform: Platform): "md" | "toml" =>
  PLATFORM_SPECS[platform].agentExtension;

export const lockFileName = (platform: Platform): string => {
  const slug = platformSlug(platform);

  return slug === "" ? "cmx-lock.json" : `cmx-lock-${slug}.json`;
};

export const platformsLabel = (platforms: readonly Platform[]): string => platforms.join(", ");
