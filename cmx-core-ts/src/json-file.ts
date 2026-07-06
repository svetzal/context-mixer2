import path from "node:path/posix";

import type { Filesystem } from "./gateway.ts";

const sortJsonValue = (value: unknown): unknown => {
  if (Array.isArray(value)) {
    return value.map(sortJsonValue);
  }

  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, nested]) => [key, sortJsonValue(nested)]),
    );
  }

  return value;
};

export const tmpPath = (targetPath: string): string =>
  path.join(path.dirname(targetPath), `${path.basename(targetPath)}.tmp`);

export const loadJson = async <T>(
  targetPath: string,
  fs: Filesystem,
  defaultValue: () => T,
): Promise<T> => {
  if (!(await fs.exists(targetPath))) {
    return defaultValue();
  }

  const content = await fs.readText(targetPath);

  try {
    return JSON.parse(content) as T;
  } catch (error) {
    throw new Error(`Failed to parse ${targetPath}`, { cause: error });
  }
};

export const saveJson = async <T>(value: T, targetPath: string, fs: Filesystem): Promise<void> => {
  const parent = path.dirname(targetPath);
  await fs.createDirAll(parent);

  const content = `${JSON.stringify(sortJsonValue(value), null, 2)}\n`;
  const tempPath = tmpPath(targetPath);
  await fs.write(tempPath, content);
  await fs.rename(tempPath, targetPath);
};
