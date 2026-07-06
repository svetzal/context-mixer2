export const INSTALL_SCOPES = ["global", "local"] as const;
export type InstallScope = (typeof INSTALL_SCOPES)[number];

export const ARTIFACT_KINDS = ["agent", "skill"] as const;
export type ArtifactKind = (typeof ARTIFACT_KINDS)[number];

export const SOURCE_TYPES = ["local", "git"] as const;
export type SourceType = (typeof SOURCE_TYPES)[number];

export interface LockSource {
  repo: string;
  path: string;
}

export interface LockEntry {
  type: ArtifactKind;
  version?: string;
  installed_at: string;
  source: LockSource;
  source_checksum: string;
  installed_checksum: string;
}

export interface LockFile {
  version: number;
  packages: Record<string, LockEntry>;
}

export interface SourceEntry {
  type: SourceType;
  path?: string;
  url?: string;
  local_clone?: string;
  branch?: string;
  last_updated?: string;
}

export interface SourcesFile {
  version: number;
  sources: Record<string, SourceEntry>;
}

export const LLM_GATEWAY_TYPES = ["openai", "ollama"] as const;
export type LlmGatewayType = (typeof LLM_GATEWAY_TYPES)[number];

export interface LlmConfig {
  gateway: LlmGatewayType;
  model: string;
}

export interface CmxConfig {
  version: number;
  llm: LlmConfig;
  home?: string;
  external: string[];
  platforms: string[];
}

export const defaultLlmConfig = (): LlmConfig => ({
  gateway: "openai",
  model: "gpt-5.4",
});

export const defaultCmxConfig = (): CmxConfig => ({
  version: 1,
  llm: defaultLlmConfig(),
  external: [],
  platforms: [],
});

export const defaultSourcesFile = (): SourcesFile => ({
  version: 1,
  sources: {},
});

export const defaultLockFile = (): LockFile => ({
  version: 1,
  packages: {},
});

export const scopeLabel = (scope: InstallScope): InstallScope => scope;

export const isLocalScope = (scope: InstallScope): boolean => scope === "local";

export const installedArtifactPath = (
  kind: ArtifactKind,
  name: string,
  dir: string,
  agentExtension: string,
): string => {
  if (kind === "skill") {
    return `${dir}/${name}`;
  }

  return `${dir}/${name}.${agentExtension}`;
};
