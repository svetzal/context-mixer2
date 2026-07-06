import { managedPlatforms } from "./config.ts";
import type { Filesystem } from "./gateway.ts";
import { loadLockFile } from "./lockfile.ts";
import type { ConfigPaths } from "./paths.ts";
import { PLATFORM_VALUES, type Platform, supportsArtifact } from "./platform.ts";
import type { ArtifactKind, InstallScope } from "./types.ts";

export interface TargetResolutionContext {
  fs: Filesystem;
  paths: ConfigPaths;
}

export const resolveTargets = async (
  selector: Platform | undefined,
  kind: ArtifactKind,
  scope: InstallScope,
  context: TargetResolutionContext,
): Promise<Platform[]> => {
  if (selector !== undefined) {
    return [selector];
  }

  const managed = await managedPlatforms(context.fs, context.paths);
  if (managed !== undefined) {
    return managed.filter((platform) => supportsArtifact(platform, kind));
  }

  const targets: Platform[] = [];
  for (const platform of PLATFORM_VALUES) {
    if (!supportsArtifact(platform, kind)) {
      continue;
    }

    const lockFile = await loadLockFile(scope, context.fs, context.paths.withPlatform(platform));
    if (Object.keys(lockFile.packages).length > 0) {
      targets.push(platform);
    }
  }

  return targets.length > 0 ? targets : ["claude"];
};
