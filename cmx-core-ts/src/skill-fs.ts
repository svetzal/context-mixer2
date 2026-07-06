import path from "node:path/posix";

import { checksumInMemory, isCanonicalRelPath, relPathKey } from "./checksum.ts";
import type { Filesystem } from "./gateway.ts";

export interface SkillFile {
  relPath: string;
  bytes: Uint8Array;
}

const textEncoder = new TextEncoder();

export const textSkillFile = (relPath: string, content: string): SkillFile => ({
  relPath,
  bytes: textEncoder.encode(content),
});

export const canonicalFiles = (files: readonly SkillFile[]): SkillFile[] =>
  [...files]
    .filter((file) => isCanonicalRelPath(file.relPath))
    .sort((left, right) => {
      const leftKey = relPathKey(left.relPath);
      const rightKey = relPathKey(right.relPath);
      return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
    });

export const checksumBundled = (files: readonly SkillFile[]): string =>
  checksumInMemory(canonicalFiles(files));

export const writeSkillFiles = async (
  destDir: string,
  files: readonly SkillFile[],
  fs: Filesystem,
): Promise<void> => {
  await fs.createDirAll(destDir);
  for (const file of files) {
    const destPath = path.join(destDir, file.relPath);
    await fs.createDirAll(path.dirname(destPath));
    await fs.writeBytes(destPath, file.bytes);
  }
};
