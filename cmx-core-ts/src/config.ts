import type { Filesystem } from "./gateway.ts";
import { loadJson, saveJson } from "./json-file.ts";
import type { ConfigPaths } from "./paths.ts";
import { isPlatform, PLATFORM_VALUES, type Platform } from "./platform.ts";
import {
  type CmxConfig,
  defaultCmxConfig,
  defaultSourcesFile,
  type SourceEntry,
  type SourcesFile,
} from "./types.ts";

const normalizeConfig = (config: CmxConfig): CmxConfig => ({
  version: config.version ?? 1,
  llm: config.llm ?? defaultCmxConfig().llm,
  home: config.home,
  external: config.external ?? [],
  platforms: (config.platforms ?? []).filter(isPlatform),
});

const normalizeSources = (sources: SourcesFile): SourcesFile => ({
  version: sources.version ?? 1,
  sources: sources.sources ?? {},
});

export const loadSources = async (fs: Filesystem, paths: ConfigPaths): Promise<SourcesFile> =>
  normalizeSources(await loadJson(paths.sourcesPath(), fs, defaultSourcesFile));

export const saveSources = async (
  sources: SourcesFile,
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<void> => {
  await saveJson(normalizeSources(sources), paths.sourcesPath(), fs);
};

export const mutateSources = async <T>(
  fs: Filesystem,
  paths: ConfigPaths,
  mutator: (sources: SourcesFile) => T | Promise<T>,
): Promise<T> => {
  const sources = await loadSources(fs, paths);
  const result = await mutator(sources);
  await saveSources(sources, fs, paths);
  return result;
};

export const loadConfig = async (fs: Filesystem, paths: ConfigPaths): Promise<CmxConfig> =>
  normalizeConfig(await loadJson(paths.configPath(), fs, defaultCmxConfig));

export const saveConfig = async (
  config: CmxConfig,
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<void> => {
  await saveJson(normalizeConfig(config), paths.configPath(), fs);
};

export const managedPlatforms = async (
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<Platform[] | undefined> => {
  const config = await loadConfig(fs, paths);
  return config.platforms.length > 0 ? ([...config.platforms] as Platform[]) : undefined;
};

export const managedOrAllPlatforms = async (
  fs: Filesystem,
  paths: ConfigPaths,
): Promise<Platform[]> => (await managedPlatforms(fs, paths)) ?? [...PLATFORM_VALUES];

export const resolveArtifactHome = (config: CmxConfig, paths: ConfigPaths): string =>
  config.home ?? paths.defaultArtifactHome();

export const resolveLocalPath = (entry: SourceEntry): string => {
  if (entry.type === "local" && entry.path !== undefined) {
    return entry.path;
  }

  if (entry.type === "git" && entry.local_clone !== undefined) {
    return entry.local_clone;
  }

  throw new Error(
    entry.type === "local"
      ? "Local source has no path configured"
      : "Git source has no local clone path configured",
  );
};
